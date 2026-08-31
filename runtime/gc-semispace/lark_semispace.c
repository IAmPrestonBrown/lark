/* The moving, copying, two space collector.
 *
 * Chapter 10 section 4 names it `semispace`. The design is Cheney's algorithm.
 * Allocation is a bump pointer in from space. A collection copies every
 * reachable object to to space, leaves a forwarding address behind, and swaps
 * the two spaces. Everything that it did not copy is dead, and the whole of
 * from space becomes free at once.
 *
 * The collector moves objects, so it needs two things that a non moving
 * collector does not.
 *
 * It needs the address of every root, not the value alone, so that it can
 * write the new address back. Rule M-10 gives that under shadow stack mode. A
 * conservative scan cannot give it, because a word that looks like a pointer
 * can be an integer, and writing to it would corrupt the program. So this
 * collector refuses to start under rule M-13 root scanning.
 *
 * It also breaks rule M-8. An interior pointer has no slot of its own, so a
 * copy leaves it pointing into dead memory. The capability flag says so, and
 * rule R-1 makes the transpiler reject a program that needs one. */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lark_internal.h"
#include "lark_rt.h"

/* The bytes that each space holds at the start. A space that fills doubles. */
#define INITIAL_SPACE ((size_t)1u * 1024u * 1024u)

/* The alignment that every object starts on. */
#define BASE_ALIGN ((size_t)16u)

/* The marker that a moved object leaves in place of its type.
 *
 * A header whose type equals this holds the new address in `count`. No real
 * type descriptor sits at this address, so the two never collide. */
static const lark_typeinfo FORWARDED = { "forwarded", 0u, 1u, 0u, NULL, 0u, NULL };

/* One half of the heap. */
typedef struct space {
    uint8_t *memory;
    size_t bytes;
    size_t used;
} space;

static space from_space;
static space to_space;
static lark_gc_stats stats;
static size_t heap_limit = INITIAL_SPACE;

/* -- Helpers ------------------------------------------------------------- */

static size_t round_up(size_t value, size_t to) {
    return (value + to - 1u) / to * to;
}

/* Reserves `bytes` for one space, and returns false when memory runs out. */
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

/* Reports whether an address points into the payload of a from space object. */
static bool in_from_space(const void *value) {
    uintptr_t address = (uintptr_t)value;
    uintptr_t low = (uintptr_t)from_space.memory;
    return address >= low && address < low + from_space.used;
}

/* -- Copying ------------------------------------------------------------- */

/* Copies one object to to space, and returns its new payload address.
 *
 * A second call for the same object reads the forwarding address rather than
 * copying again, so a shared object stays shared and a cycle terminates. */
static void *evacuate(void *payload) {
    lark_header *header = LARK_HEADER(payload);
    if (header->type == &FORWARDED) {
        return (void *)header->count;
    }

    size_t bytes = header->type->size * header->count;
    size_t need = round_up(sizeof(lark_header) + bytes, BASE_ALIGN);
    /* To space is the same size as from space, and it holds only what from
     * space held, so this never runs out. The check states that. */
    if (to_space.used + need > to_space.bytes) {
        fputs("lark: to space overflowed during a collection\n", stderr);
        abort();
    }

    uint8_t *block = to_space.memory + to_space.used;
    to_space.used += need;
    memcpy(block, header, sizeof(lark_header) + bytes);

    void *moved = block + sizeof(lark_header);
    /* Leave the forwarding address where the old header was. */
    header->type = &FORWARDED;
    header->count = (uintptr_t)moved;
    return moved;
}

/* Visits one root. The core supplies the slot, and this writes the new
 * address back into it. */
static void move_root(void *value, void **slot, void *ctx) {
    (void)ctx;
    if (value == NULL || !in_from_space(value)) {
        return;
    }
    if (slot == NULL) {
        /* A conservative scan produced this value, and `semispace_init`
         * refuses that mode, so the case cannot arise. */
        fputs("lark: the moving collector needs the address of every root\n", stderr);
        abort();
    }
    *slot = evacuate(value);
}

/* Walks the objects that the copy already placed in to space, and moves what
 * their managed fields point to. Cheney's scan pointer is `scanned`. */
static void scan_to_space(void) {
    size_t scanned = 0u;
    while (scanned < to_space.used) {
        uint8_t *block = to_space.memory + scanned;
        lark_header *header = (lark_header *)(void *)block;
        const lark_typeinfo *type = header->type;
        uint8_t *payload = block + sizeof(lark_header);

        /* Rule M-5. The field map lists the byte offset of every managed
         * field, so the scan needs no guess. */
        for (uintptr_t item = 0u; item < header->count; item += 1u) {
            uint8_t *element = payload + item * type->size;
            for (uint32_t index = 0u; index < type->nptrs; index += 1u) {
                void **field = (void **)(void *)(element + type->ptr_offsets[index]);
                if (*field != NULL && in_from_space(*field)) {
                    *field = evacuate(*field);
                }
            }
        }

        scanned += round_up(sizeof(lark_header) + type->size * header->count, BASE_ALIGN);
    }
}

/* -- The collector interface --------------------------------------------- */

static void semispace_init(const lark_gc_config *value) {
    if (value->roots != LARK_ROOTS_SHADOW_STACK) {
        fputs("lark: the semispace collector needs `gc.roots = \"shadow-stack\"`.\n"
              "      A moving collector must write a new address into every root,\n"
              "      and a conservative scan cannot say which words are roots.\n",
              stderr);
        exit(2);
    }
    memset(&stats, 0, sizeof stats);
    heap_limit = value->heap_limit != 0u ? value->heap_limit : INITIAL_SPACE;
    size_t bytes = round_up(heap_limit, BASE_ALIGN);
    if (!space_open(&from_space, bytes) || !space_open(&to_space, bytes)) {
        fputs("lark: cannot reserve the two spaces\n", stderr);
        exit(2);
    }
    stats.heap_bytes = from_space.bytes + to_space.bytes;
}

static void semispace_shutdown(void) {
    space_close(&from_space);
    space_close(&to_space);
    memset(&stats, 0, sizeof stats);
}

/* Enlarges the empty destination space. Defined below. */
static bool reserve(size_t wanted);

static void semispace_collect(void) {
    to_space.used = 0u;

    /* Everything live can survive, so the destination must hold what from
     * space holds. The two spaces swap at the end of every collection, so they
     * do not stay the same size, and a program that calls `lark_collect`
     * itself reaches this with the smaller one as the destination.
     *
     * The space is empty here, so enlarging it moves no object. */
    if (to_space.bytes < from_space.used) {
        (void)reserve(from_space.used);
    }

    lark_visit_roots(move_root, NULL);
    scan_to_space();

    /* Swap the spaces. Everything left in the old from space is dead. */
    space swap = from_space;
    from_space = to_space;
    to_space = swap;
    /* The next collection reuses the old space, so clear it. Clearing also
     * turns a stale pointer into a read of zeroed memory under a sanitizer,
     * which turns a silent fault into a loud one. */
    memset(to_space.memory, 0, to_space.bytes);
    to_space.used = 0u;

    stats.collections += 1;
    stats.live_bytes = from_space.used;
    stats.live_objects = 0u;
    size_t offset = 0u;
    while (offset < from_space.used) {
        const lark_header *header = (const lark_header *)(void *)(from_space.memory + offset);
        stats.live_objects += 1u;
        offset += round_up(sizeof(lark_header) + header->type->size * header->count, BASE_ALIGN);
    }
}

/* Enlarges the space that the next collection copies into.
 *
 * `to_space` holds nothing between two collections, so replacing it moves no
 * object and breaks no pointer. The collection that follows copies every live
 * object into it and rewrites every root and every field, and that is what
 * makes the new addresses correct.
 *
 * A raw copy of `from_space` cannot do the same. The objects would sit at new
 * addresses and nothing would update the pointers that name them, so the heap
 * would read as garbage.
 *
 * Returns false when the memory is not available, and leaves the space as it
 * was. */
static bool reserve(size_t wanted) {
    size_t bytes = to_space.bytes;
    while (bytes < wanted) {
        size_t doubled = bytes * 2u;
        if (doubled <= bytes) {
            return false;
        }
        bytes = doubled;
    }
    if (bytes == to_space.bytes) {
        return true;
    }
    space grown;
    if (!space_open(&grown, bytes)) {
        return false;
    }
    space_close(&to_space);
    to_space = grown;
    stats.heap_bytes = from_space.bytes + to_space.bytes;
    return true;
}

static void *semispace_alloc(const lark_typeinfo *type, size_t count) {
    if (count == 0u) {
        count = 1u;
    }
    size_t payload = type->size * count;
    size_t need = round_up(sizeof(lark_header) + payload, BASE_ALIGN);

    if (from_space.used + need > from_space.bytes) {
        /* Everything live can survive, so the destination must hold what from
         * space holds plus this request. The reservation doubles that, so the
         * space that the collection leaves has at least as much room free as
         * it has live data.
         *
         * Without the doubling, a collection that frees little leaves the
         * space nearly full and the next allocation collects again. Each of
         * those collections copies the whole live set, so the program stops
         * making progress. */
        size_t bound = from_space.used + need;
        if (!reserve(bound * 2u) && !reserve(bound)) {
            return NULL;
        }
        lark_collect();
        if (from_space.used + need > from_space.bytes) {
            return NULL;
        }
    }

    uint8_t *block = from_space.memory + from_space.used;
    from_space.used += need;

    lark_header *header = (lark_header *)(void *)block;
    header->type = type;
    header->count = count;
    void *result = block + sizeof(lark_header);
    /* Rule O-5. A managed field must never hold garbage when a collection
     * starts, and a reused space holds whatever the last cycle left. */
    memset(result, 0, payload);

    stats.total_allocations += 1;
    return result;
}

/* Rule M-8. A moving collector cannot follow an interior pointer, because the
 * pointer has no slot that a copy can update. */
static void *semispace_base_of(void *interior) {
    (void)interior;
    return NULL;
}

static lark_gc_stats semispace_statistics(void) {
    lark_gc_stats current = stats;
    current.live_bytes = from_space.used;
    return current;
}

static const lark_collector SEMISPACE = {
    .name = "semispace",
    .caps = {
        .interior_pointers = false,  /* rule M-8 does not hold here */
        .moving = true,
        .precise_heap = true,
        .precise_stack = true,       /* it accepts no other mode */
        .concurrent = false,
        .reclaims = true,
    },
    .init = semispace_init,
    .shutdown = semispace_shutdown,
    .alloc = semispace_alloc,
    .collect = semispace_collect,
    .base_of = semispace_base_of,
    .statistics = semispace_statistics,
};

const lark_collector *lark_gc_collector(void) {
    return &SEMISPACE;
}
