/* The arena collector. It allocates and never frees.
 *
 * Chapter 10 section 4 names it `arena`. The design is a list of chunks and a
 * bump pointer. `collect` walks no object and frees no memory, so a program
 * that runs under it holds every object it ever allocated.
 *
 * The collector exists for three reasons. It is the smallest plugin that the
 * seam accepts, so it proves that the seam is a real boundary. It gives a
 * baseline for the cost of collection, because a program that runs under it
 * pays for allocation alone. It removes the collector from a bug hunt, because
 * a fault that survives here is not a collector fault. */

#include <stdlib.h>
#include <string.h>

#include "lark_internal.h"
#include "lark_rt.h"

/* The bytes that one chunk holds. A larger object gets a chunk of its own. */
#define CHUNK_SIZE ((size_t)256u * 1024u)

/* The alignment that every object starts on. It covers `max_align_t` on every
 * platform that Lark targets, so a payload after a header stays aligned. */
#define BASE_ALIGN ((size_t)16u)

/* One run of memory that the bump pointer walks. */
typedef struct chunk {
    struct chunk *next;
    uint8_t *memory;         /* the whole allocation */
    size_t bytes;            /* how large the allocation is */
    size_t used;             /* how much of it the bump pointer passed */
} chunk;

static chunk *chunks;
static lark_gc_stats stats;

/* -- Helpers ------------------------------------------------------------- */

static size_t round_up(size_t value, size_t to) {
    return (value + to - 1u) / to * to;
}

/* Adds a chunk large enough for `need` bytes, and returns it. */
static chunk *chunk_new(size_t need) {
    size_t bytes = need > CHUNK_SIZE ? round_up(need, BASE_ALIGN) : CHUNK_SIZE;
    chunk *owner = calloc(1, sizeof *owner);
    if (owner == NULL) {
        return NULL;
    }
    owner->memory = calloc(1, bytes + BASE_ALIGN);
    if (owner->memory == NULL) {
        free(owner);
        return NULL;
    }
    /* The first object starts on a base aligned address. */
    size_t skew = (size_t)((uintptr_t)owner->memory % BASE_ALIGN);
    owner->used = skew == 0u ? 0u : BASE_ALIGN - skew;
    owner->bytes = bytes + BASE_ALIGN;
    owner->next = chunks;
    chunks = owner;
    stats.heap_bytes += owner->bytes;
    return owner;
}

/* -- The collector interface --------------------------------------------- */

static void arena_init(const lark_gc_config *value) {
    (void)value;
    chunks = NULL;
    memset(&stats, 0, sizeof stats);
}

static void arena_shutdown(void) {
    chunk *owner = chunks;
    while (owner != NULL) {
        chunk *next = owner->next;
        free(owner->memory);
        free(owner);
        owner = next;
    }
    chunks = NULL;
    memset(&stats, 0, sizeof stats);
}

static void *arena_alloc(const lark_typeinfo *type, size_t count) {
    if (count == 0u) {
        count = 1u;
    }
    size_t payload = type->size * count;
    size_t need = round_up(sizeof(lark_header) + payload, BASE_ALIGN);

    chunk *owner = chunks;
    if (owner == NULL || owner->bytes - owner->used < need) {
        owner = chunk_new(need);
        if (owner == NULL) {
            return NULL;
        }
    }

    uint8_t *block = owner->memory + owner->used;
    owner->used += need;

    lark_header *header = (lark_header *)(void *)block;
    header->type = type;
    header->count = count;
    void *result = block + sizeof(lark_header);
    /* Rule O-5. A field with no designator is zero. `calloc` cleared the
     * chunk, and nothing here is ever reused, so the memory is already zero.
     * The write states the rule rather than trusting the allocator. */
    memset(result, 0, payload);

    stats.total_allocations += 1;
    stats.live_objects += 1;
    stats.live_bytes += payload;
    return result;
}

static void arena_collect(void) {
    /* Nothing dies here. The count still rises, so a test can prove that a
     * safepoint reached the collector. */
    stats.collections += 1;
}

/* Rule M-7. The scan walks the chunk that holds the address, because every
 * object in a chunk sits next to the one before it. */
static void *arena_base_of(void *interior) {
    if (interior == NULL) {
        return NULL;
    }
    uintptr_t address = (uintptr_t)interior;
    for (const chunk *owner = chunks; owner != NULL; owner = owner->next) {
        uintptr_t low = (uintptr_t)owner->memory;
        uintptr_t high = low + owner->used;
        if (address < low || address >= high) {
            continue;
        }
        size_t offset = 0u;
        /* Skip the alignment skew that the first object started after. */
        size_t skew = (size_t)(low % BASE_ALIGN);
        if (skew != 0u) {
            offset = BASE_ALIGN - skew;
        }
        while (offset < owner->used) {
            uint8_t *block = owner->memory + offset;
            const lark_header *header = (const lark_header *)(void *)block;
            if (header->type == NULL) {
                break;
            }
            size_t payload = header->type->size * header->count;
            size_t step = round_up(sizeof(lark_header) + payload, BASE_ALIGN);
            uintptr_t start = (uintptr_t)(block + sizeof(lark_header));
            if (address >= start && address < start + payload) {
                return (void *)start;
            }
            offset += step;
        }
        return NULL;
    }
    return NULL;
}

static lark_gc_stats arena_statistics(void) {
    return stats;
}

static const lark_collector ARENA = {
    .name = "arena",
    .caps = {
        .interior_pointers = true,
        .moving = false,
        .precise_heap = true,
        .precise_stack = true,
        .concurrent = false,
        .reclaims = false,   /* nothing ever dies here */
    },
    .init = arena_init,
    .shutdown = arena_shutdown,
    .alloc = arena_alloc,
    .collect = arena_collect,
    .base_of = arena_base_of,
    .statistics = arena_statistics,
};

const lark_collector *lark_gc_collector(void) {
    return &ARENA;
}
