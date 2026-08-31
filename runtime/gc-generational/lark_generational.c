/* The generational, copying collector.
 *
 * Chapter 10 section 4 names it `generational`. The design is a nursery and
 * one old generation, with a card table for the remembered set.
 *
 * Most objects die young. A minor collection walks the nursery alone, which is
 * a small part of the heap, so it costs a small part of a full collection.
 *
 * That saving needs one thing that a full collection does not: a way to find a
 * pointer from an old object to a young one. Nothing in the nursery reaches
 * it, and the minor collection does not walk the old generation, so the
 * pointer would look unreachable and the young object would go.
 *
 * The card table answers that. The old generation is divided into cards, and a
 * store of a managed pointer marks the card that holds the field. A minor
 * collection then scans every marked card as a root. Rule R-2 makes the
 * transpiler emit the call, and only for a collector that asks for one.
 *
 * The collector moves objects, so it needs the same two things as
 * `semispace`: the address of every root, which rule M-10 supplies, and no
 * interior pointer, which rule M-8 leaves to the capability. */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lark_internal.h"
#include "lark_rt.h"

/* The bytes that the nursery holds. A minor collection walks this much. */
#define NURSERY_BYTES ((size_t)256u * 1024u)

/* The largest nursery that the collector grows to on its own. */
#define NURSERY_MAX ((size_t)16u * 1024u * 1024u)

/* The share of the old generation that the nursery grows to match.
 *
 * A nursery of a fixed size collects once per that many bytes allocated,
 * whatever the live set is. Each of those collections scans the card table
 * over the whole old generation, so a program with a large live set spends
 * nearly all its time in collection. A nursery that follows the old generation
 * keeps the number of collections proportional to the heap instead. */
#define NURSERY_SHARE 4u

/* The bytes that each half of the old generation holds at the start. */
#define OLD_BYTES ((size_t)1u * 1024u * 1024u)

/* The bytes that one card covers. A store marks one byte per card. */
#define CARD_BYTES ((size_t)512u)

/* The alignment that every object starts on. */
#define BASE_ALIGN ((size_t)16u)

/* The marker that a moved object leaves in place of its type. */
static const lark_typeinfo FORWARDED = { "forwarded", 0u, 1u, 0u, NULL, 0u, NULL };

/* One run of memory that a bump pointer walks. */
typedef struct space {
    uint8_t *memory;
    size_t bytes;
    size_t used;
} space;

static space nursery;
/* The old generation is two halves, so a major collection copies between
 * them. A minor collection copies into `old` alone. */
static space old;
static space old_reserve;

/* One byte per card of the old generation. A store marks its card. */
static uint8_t *cards;
static size_t card_count;

static lark_gc_stats stats;
static size_t minor_collections;
static size_t major_collections;

/* -- Helpers ------------------------------------------------------------- */

static size_t round_up(size_t value, size_t to) {
    return (value + to - 1u) / to * to;
}

static bool space_open(space *target, size_t bytes) {
    target->memory = calloc(1, bytes);
    if (target->memory == NULL) {
        return false;
    }
    target->bytes = bytes;
    target->used = 0u;
    return true;
}

static void space_close(space *target) {
    free(target->memory);
    target->memory = NULL;
    target->bytes = 0u;
    target->used = 0u;
}

static bool in_space(const space *target, const void *value) {
    uintptr_t address = (uintptr_t)value;
    uintptr_t low = (uintptr_t)target->memory;
    return target->memory != NULL && address >= low && address < low + target->used;
}

/* Returns the card that holds an address in the old generation. */
static size_t card_of(const void *value) {
    uintptr_t offset = (uintptr_t)value - (uintptr_t)old.memory;
    return (size_t)offset / CARD_BYTES;
}

/* -- Copying ------------------------------------------------------------- */

/* Where a copy goes. A minor collection promotes to the old generation, and a
 * major one copies within it. */
static space *destination;

/* Whether the collection walks the old generation as well as the nursery. */
static bool major;

/* Reports whether a value needs a copy in this collection. */
static bool needs_copy(const void *value) {
    if (in_space(&nursery, value)) {
        return true;
    }
    return major && in_space(&old, value);
}

/* Copies one object, and returns its new payload address. */
static void *evacuate(void *payload) {
    lark_header *header = LARK_HEADER(payload);
    if (header->type == &FORWARDED) {
        return (void *)header->count;
    }

    size_t bytes = header->type->size * header->count;
    size_t need = round_up(sizeof(lark_header) + bytes, BASE_ALIGN);
    if (destination->used + need > destination->bytes) {
        /* `ensure_room` reserved space for everything that this collection can
         * copy, so this cannot happen. A copy is half done here and no caller
         * can recover, so the check stops rather than corrupts. Rule R-8 keeps
         * the recoverable case in the allocation path, which returns NULL. */
        fputs("lark: the destination filled during a collection\n", stderr);
        abort();
    }

    uint8_t *block = destination->memory + destination->used;
    destination->used += need;
    memcpy(block, header, sizeof(lark_header) + bytes);

    void *moved = block + sizeof(lark_header);
    header->type = &FORWARDED;
    header->count = (uintptr_t)moved;
    return moved;
}

/* Visits one root and writes the new address back into its slot. */
static void move_root(void *value, void **slot, void *ctx) {
    (void)ctx;
    if (value == NULL || !needs_copy(value)) {
        return;
    }
    if (slot == NULL) {
        fputs("lark: the generational collector needs the address of every root\n", stderr);
        abort();
    }
    *slot = evacuate(value);
}

/* Moves every managed field of one object that this collection copies. */
static void scan_object(lark_header *header, uint8_t *payload) {
    const lark_typeinfo *type = header->type;
    for (uintptr_t item = 0u; item < header->count; item += 1u) {
        uint8_t *element = payload + item * type->size;
        for (uint32_t index = 0u; index < type->nptrs; index += 1u) {
            void **field = (void **)(void *)(element + type->ptr_offsets[index]);
            if (*field != NULL && needs_copy(*field)) {
                *field = evacuate(*field);
            }
        }
    }
}

/* Walks what the copy already placed, and moves what it points at. */
static void scan_destination(size_t from) {
    size_t scanned = from;
    while (scanned < destination->used) {
        uint8_t *block = destination->memory + scanned;
        lark_header *header = (lark_header *)(void *)block;
        scan_object(header, block + sizeof(lark_header));
        scanned += round_up(
            sizeof(lark_header) + header->type->size * header->count, BASE_ALIGN);
    }
}

/* Scans every marked card of the old generation as a root.
 *
 * A card holds the fields that a store marked. The scan walks the objects that
 * the card covers, because a card is a range of bytes rather than a list of
 * fields. That is the trade the card table makes: one byte per card, and a
 * short walk when it is marked. */
static void scan_marked_cards(void) {
    size_t offset = 0u;
    size_t index = 0u;
    while (offset < old.used) {
        uint8_t *block = old.memory + offset;
        lark_header *header = (lark_header *)(void *)block;
        size_t step = round_up(
            sizeof(lark_header) + header->type->size * header->count, BASE_ALIGN);

        size_t first = offset / CARD_BYTES;
        size_t last = (offset + step - 1u) / CARD_BYTES;
        bool marked = false;
        for (size_t card = first; card <= last && card < card_count; card += 1u) {
            if (cards[card] != 0u) {
                marked = true;
                break;
            }
        }
        if (marked) {
            scan_object(header, block + sizeof(lark_header));
        }

        offset += step;
        index += 1u;
    }
    (void)index;
}

/* -- The collector interface --------------------------------------------- */

static void generational_init(const lark_gc_config *value) {
    if (value->roots != LARK_ROOTS_SHADOW_STACK) {
        fputs("lark: the generational collector needs `gc.roots = \"shadow-stack\"`.\n"
              "      A moving collector must write a new address into every root,\n"
              "      and a conservative scan cannot say which words are roots.\n",
              stderr);
        exit(2);
    }
    memset(&stats, 0, sizeof stats);
    minor_collections = 0u;
    major_collections = 0u;

    size_t old_bytes = value->heap_limit != 0u ? value->heap_limit : OLD_BYTES;
    old_bytes = round_up(old_bytes, CARD_BYTES);
    if (!space_open(&nursery, NURSERY_BYTES) || !space_open(&old, old_bytes)
        || !space_open(&old_reserve, old_bytes)) {
        fputs("lark: cannot reserve the generations\n", stderr);
        exit(2);
    }

    card_count = old_bytes / CARD_BYTES;
    cards = calloc(card_count, 1);
    if (cards == NULL) {
        fputs("lark: cannot reserve the card table\n", stderr);
        exit(2);
    }
    stats.heap_bytes = nursery.bytes + old.bytes + old_reserve.bytes;
}

static void generational_shutdown(void) {
    space_close(&nursery);
    space_close(&old);
    space_close(&old_reserve);
    free(cards);
    cards = NULL;
    card_count = 0u;
    memset(&stats, 0, sizeof stats);
    minor_collections = 0u;
    major_collections = 0u;
}

/* Counts the objects of one space, for the statistics. */
static size_t count_objects(const space *target) {
    size_t offset = 0u;
    size_t found = 0u;
    while (offset < target->used) {
        const lark_header *header = (const lark_header *)(const void *)(target->memory + offset);
        found += 1u;
        offset += round_up(
            sizeof(lark_header) + header->type->size * header->count, BASE_ALIGN);
    }
    return found;
}

static void record_stats(void) {
    stats.live_objects = count_objects(&old) + count_objects(&nursery);
    stats.live_bytes = old.used + nursery.used;
    stats.collections = minor_collections + major_collections;
    /* Every byte the collector holds, so a reader can compare this collector
     * with one that keeps a single heap. */
    stats.heap_bytes = nursery.bytes + old.bytes + old_reserve.bytes;
}

/* How far the old generation grows past what a collection leaves in it. */
#define GROWTH_FACTOR 2u

/* Grows the card table to cover the old generation. */
static bool cards_resize(size_t old_bytes) {
    size_t wanted = old_bytes / CARD_BYTES;
    if (wanted <= card_count) {
        return true;
    }
    uint8_t *grown = calloc(wanted, 1);
    if (grown == NULL) {
        return false;
    }
    free(cards);
    cards = grown;
    card_count = wanted;
    return true;
}

/* Makes room for everything that the next collection can copy.
 *
 * A minor collection promotes into `old` after what it already holds, and it
 * cannot grow `old` while it copies: every pointer into that space would move
 * under the program. `old_reserve` holds nothing between collections, so it
 * grows freely. A collection that does not fit therefore grows the reserve and
 * runs as a major one, which copies into the reserve and swaps.
 *
 * The bound is exact. Everything live sits in the nursery or in the old
 * generation, so `old.used + nursery.used` is what a copy can need at most.
 *
 * Sets `*full` when the collection must become a major one. Returns false when
 * the memory is not available, and the caller then runs the collection it
 * planned, which either fits or reports the overflow. */
static bool ensure_room(bool *full) {
    size_t bound = old.used + nursery.used;
    if (!*full && old.bytes - old.used >= nursery.used) {
        return true;
    }

    /* A minor collection that does not fit becomes a major one, which is the
     * only kind that can leave the old generation larger than it was. */
    *full = true;
    if (old_reserve.bytes >= bound) {
        return true;
    }

    size_t bytes = old_reserve.bytes == 0u ? OLD_BYTES : old_reserve.bytes;
    while (bytes < bound * GROWTH_FACTOR) {
        size_t doubled = bytes * 2u;
        if (doubled <= bytes) {
            return false;
        }
        bytes = doubled;
    }
    space grown;
    if (!space_open(&grown, bytes)) {
        return false;
    }
    if (!cards_resize(bytes)) {
        space_close(&grown);
        return false;
    }
    space_close(&old_reserve);
    old_reserve = grown;
    return true;
}

/* Runs one collection. A minor one walks the nursery, and a major one walks
 * the old generation as well. */
static void run_collection(bool full) {
    major = full;
    size_t scan_from;

    if (full) {
        /* A major collection copies the whole live set into the reserve. */
        old_reserve.used = 0u;
        destination = &old_reserve;
        scan_from = 0u;
    } else {
        /* A minor collection promotes into the old generation, after what it
         * already holds. */
        destination = &old;
        scan_from = old.used;
    }

    lark_visit_roots(move_root, NULL);
    if (!full) {
        /* Rule R-2. A pointer from an old object to a young one is a root
         * that nothing else finds. */
        scan_marked_cards();
    }
    scan_destination(scan_from);

    if (full) {
        space swap = old;
        old = old_reserve;
        old_reserve = swap;
        memset(old_reserve.memory, 0, old_reserve.bytes);
        old_reserve.used = 0u;
        /* The two halves must match, because either one can be the
         * destination of the next major collection. */
        if (old_reserve.bytes < old.bytes) {
            space grown;
            if (space_open(&grown, old.bytes)) {
                space_close(&old_reserve);
                old_reserve = grown;
            }
        }
        major_collections += 1u;
    } else {
        minor_collections += 1u;
    }

    /* Every card is clear after a collection, because every marked field was
     * scanned and every young object it named is now old. */
    memset(cards, 0, card_count);

    nursery.used = 0u;
    memset(nursery.memory, 0, nursery.bytes);

    /* The nursery is empty here, so replacing it moves no object. */
    size_t wanted = old.used / NURSERY_SHARE;
    if (wanted > NURSERY_MAX) {
        wanted = NURSERY_MAX;
    }
    if (wanted > nursery.bytes) {
        space grown;
        if (space_open(&grown, wanted)) {
            space_close(&nursery);
            nursery = grown;
        }
    }
    record_stats();
}

/* Whether the next collection walks the old generation as well.
 *
 * A program that calls `lark_collect` wants the whole heap swept, so the
 * default is a full collection. A nursery that fills asks for a minor one, and
 * it does so by setting this before it calls `lark_collect`. */
static bool want_major = true;

static void generational_collect(void) {
    bool full = want_major;
    want_major = true;
    /* The space has to exist before the copy starts, because a copy that runs
     * out of room has nowhere to put the object it holds. */
    (void)ensure_room(&full);
    run_collection(full);
}

/* Runs a minor collection, with the world stopped.
 *
 * Rule M-26 stops every other thread before a collection starts. A collector
 * that moves objects must not run while another thread reads one, so this goes
 * through `lark_collect` rather than calling the collection itself. */
static void collect_minor(void) {
    want_major = false;
    lark_collect();
    want_major = true;
}

static void *generational_alloc(const lark_typeinfo *type, size_t count) {
    if (count == 0u) {
        count = 1u;
    }
    size_t payload = type->size * count;
    size_t need = round_up(sizeof(lark_header) + payload, BASE_ALIGN);

    /* An object larger than the nursery goes straight to the old generation. */
    if (need > nursery.bytes) {
        if (old.used + need > old.bytes) {
            lark_collect();
        }
        if (old.used + need > old.bytes) {
            return NULL;
        }
        uint8_t *block = old.memory + old.used;
        old.used += need;
        lark_header *header = (lark_header *)(void *)block;
        header->type = type;
        header->count = count;
        void *result = block + sizeof(lark_header);
        memset(result, 0, payload);
        stats.total_allocations += 1;
        return result;
    }

    if (nursery.used + need > nursery.bytes) {
        /* The nursery is full, so a minor collection empties it. A major one
         * follows when the old generation is close to full as well. Both go
         * through `lark_collect`, so rule M-26 stops the world first. */
        collect_minor();
        if (old.used * 4u > old.bytes * 3u) {
            lark_collect();
        }
        if (nursery.used + need > nursery.bytes) {
            return NULL;
        }
    }

    uint8_t *block = nursery.memory + nursery.used;
    nursery.used += need;

    lark_header *header = (lark_header *)(void *)block;
    header->type = type;
    header->count = count;
    void *result = block + sizeof(lark_header);
    /* Rule O-5. A managed field must never hold garbage when a collection
     * starts, and a reused nursery holds whatever the last cycle left. */
    memset(result, 0, payload);

    stats.total_allocations += 1;
    return result;
}

/* Rule M-8. A moving collector cannot follow an interior pointer. */
static void *generational_base_of(void *interior) {
    (void)interior;
    return NULL;
}

static lark_gc_stats generational_statistics(void) {
    lark_gc_stats current = stats;
    current.live_objects = count_objects(&old) + count_objects(&nursery);
    current.live_bytes = old.used + nursery.used;
    current.collections = minor_collections + major_collections;
    return current;
}

/* Rule R-2. The store happens here, and the card that holds the field is
 * marked when the field lives in the old generation.
 *
 * A field in the nursery needs no mark, because a minor collection walks the
 * whole nursery anyway. */
static void generational_write_barrier(void **slot, void *value) {
    *slot = value;
    if (cards == NULL || !in_space(&old, slot)) {
        return;
    }
    size_t card = card_of(slot);
    if (card < card_count) {
        cards[card] = 1u;
    }
}

static const lark_collector GENERATIONAL = {
    .name = "generational",
    .caps = {
        .interior_pointers = false,  /* rule M-8 does not hold here */
        .moving = true,
        .precise_heap = true,
        .precise_stack = true,       /* it accepts no other mode */
        .concurrent = false,
        .reclaims = true,
        .write_barrier = true,       /* rule R-2 */
    },
    .init = generational_init,
    .shutdown = generational_shutdown,
    .alloc = generational_alloc,
    .collect = generational_collect,
    .base_of = generational_base_of,
    .statistics = generational_statistics,
    .write_barrier = generational_write_barrier,
};

const lark_collector *lark_gc_collector(void) {
    return &GENERATIONAL;
}
