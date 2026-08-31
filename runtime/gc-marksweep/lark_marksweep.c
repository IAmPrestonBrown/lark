/* The precise, non moving, mark and sweep collector.
 *
 * Chapter 10 section 4 names it `precise-marksweep`. The design is size
 * classed pages, a block map for rule M-7, a mark bitmap, and a stop the world
 * mark and sweep. */

#include <stdlib.h>
#include <string.h>

#include "lark_internal.h"
#include "lark_rt.h"

/* Every page is this many bytes, and starts at a multiple of it.
 *
 * The alignment is what makes rule M-7 constant time: masking any address
 * gives the page that holds it. */
#define PAGE_SIZE ((size_t)64u * 1024u)
#define PAGE_MASK (~(uintptr_t)(PAGE_SIZE - 1u))

/* The heap size that triggers the first collection. */
#define DEFAULT_LIMIT ((size_t)1u * 1024u * 1024u)

/* How far the heap grows between two collections.
 *
 * A fixed limit collects once per allocation as soon as the heap passes it,
 * and each of those collections frees nothing. The limit therefore follows the
 * heap: a collection that leaves N bytes runs the next one at N times this
 * factor, so the work between two collections stays proportional to the heap.
 * The cost is that a heap holds up to this multiple of what it needs. */
#define GROWTH_FACTOR 2u

/* The block sizes that a small object uses, in bytes, header included. */
static const size_t SIZE_CLASSES[] = {
    32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1024, 1536, 2048, 4096, 8192
};
#define CLASS_COUNT (sizeof SIZE_CLASSES / sizeof SIZE_CLASSES[0])

/* One run of blocks of one size. A large object gets a page of its own. */
typedef struct page {
    struct page *next;
    size_t bytes;            /* the whole allocation, a multiple of PAGE_SIZE */
    size_t block_size;
    uint32_t block_count;
    uint32_t used;
    /* Where the next search for a free block starts.
     *
     * A search that always began at block zero re-read every block it had
     * already filled, so filling one page cost work proportional to the square
     * of the block count. A sweep frees blocks below the cursor, so it puts
     * the cursor back to zero. */
    uint32_t cursor;
    uint8_t *blocks;
    uint64_t *alloc_bits;
    uint64_t *mark_bits;
} page;

/* One entry of the page table, which maps an address to the page that owns it. */
typedef struct page_slot {
    uintptr_t base;          /* the aligned address, 0 when the slot is free */
    page *owner;
} page_slot;

static page *pages[CLASS_COUNT];

/* Where the next allocation of each size class starts looking.
 *
 * A search that always began at the head of the list walked every full page
 * that sits ahead of the first free block. After a sweep the free blocks are
 * spread through the list, so that walk cost work proportional to the page
 * count on every allocation.
 *
 * A sweep frees blocks anywhere in the list, and it can free a whole page, so
 * it clears every cursor. A cleared cursor means the head of the list. */
static page *alloc_cursor[CLASS_COUNT];
static page *large_pages;
static page_slot *page_table;
static size_t page_table_capacity;
static size_t page_table_used;

static void **worklist;
static size_t worklist_length;
static size_t worklist_capacity;

static lark_gc_stats stats;
static size_t heap_limit = DEFAULT_LIMIT;

/* The limit that the configuration asked for, which is the floor. */
static size_t configured_limit = DEFAULT_LIMIT;

/* -- Bit helpers --------------------------------------------------------- */

static bool bit_get(const uint64_t *bits, uint32_t index) {
    return (bits[index / 64u] & ((uint64_t)1u << (index % 64u))) != 0u;
}

static void bit_set(uint64_t *bits, uint32_t index) {
    bits[index / 64u] |= (uint64_t)1u << (index % 64u);
}

static void bit_clear(uint64_t *bits, uint32_t index) {
    bits[index / 64u] &= ~((uint64_t)1u << (index % 64u));
}

/* -- The page table ------------------------------------------------------ */

/* Mixes an address into a table index. */
static size_t page_hash(uintptr_t base) {
    uintptr_t value = base >> 16;
    value ^= value >> 15;
    value *= (uintptr_t)0x2545F4914F6CDD1Du;
    value ^= value >> 21;
    return (size_t)value;
}

static void page_table_insert(uintptr_t base, page *owner);

/* Grows the table and reinserts every entry. */
static void page_table_grow(void) {
    size_t old_capacity = page_table_capacity;
    page_slot *old = page_table;

    size_t capacity = old_capacity == 0 ? 64u : old_capacity * 2u;
    page_slot *fresh = calloc(capacity, sizeof *fresh);
    if (fresh == NULL) {
        abort();
    }
    page_table = fresh;
    page_table_capacity = capacity;
    page_table_used = 0;

    for (size_t index = 0; index < old_capacity; index += 1) {
        if (old[index].base != 0u) {
            page_table_insert(old[index].base, old[index].owner);
        }
    }
    free(old);
}

static void page_table_insert(uintptr_t base, page *owner) {
    if (page_table_capacity == 0u || (page_table_used + 1u) * 10u > page_table_capacity * 7u) {
        page_table_grow();
    }
    size_t mask = page_table_capacity - 1u;
    size_t index = page_hash(base) & mask;
    while (page_table[index].base != 0u && page_table[index].base != base) {
        index = (index + 1u) & mask;
    }
    if (page_table[index].base == 0u) {
        page_table_used += 1;
    }
    page_table[index].base = base;
    page_table[index].owner = owner;
}

/* Removes an address from the table.
 *
 * The table uses linear probing, so a removal reinserts the run that follows.
 */
static void page_table_remove(uintptr_t base) {
    if (page_table_capacity == 0u) {
        return;
    }
    size_t mask = page_table_capacity - 1u;
    size_t index = page_hash(base) & mask;
    while (page_table[index].base != 0u && page_table[index].base != base) {
        index = (index + 1u) & mask;
    }
    if (page_table[index].base != base) {
        return;
    }
    page_table[index].base = 0u;
    page_table[index].owner = NULL;
    page_table_used -= 1;

    size_t next = (index + 1u) & mask;
    while (page_table[next].base != 0u) {
        uintptr_t moved = page_table[next].base;
        page *owner = page_table[next].owner;
        page_table[next].base = 0u;
        page_table[next].owner = NULL;
        page_table_used -= 1;
        page_table_insert(moved, owner);
        next = (next + 1u) & mask;
    }
}

/* Returns the page that owns an address, or NULL. */
static page *page_of(const void *address) {
    if (page_table_capacity == 0u) {
        return NULL;
    }
    uintptr_t base = (uintptr_t)address & PAGE_MASK;
    size_t mask = page_table_capacity - 1u;
    size_t index = page_hash(base) & mask;
    while (page_table[index].base != 0u) {
        if (page_table[index].base == base) {
            return page_table[index].owner;
        }
        index = (index + 1u) & mask;
    }
    return NULL;
}

/* -- Pages --------------------------------------------------------------- */

/* Rounds a value up to a multiple of `to`. */
static size_t round_up(size_t value, size_t to) {
    return (value + to - 1u) / to * to;
}

/* Registers every aligned page of an allocation, so rule M-7 holds for an
 * address anywhere inside a large object. */
static void register_pages(page *owner) {
    uintptr_t base = (uintptr_t)owner;
    for (size_t offset = 0; offset < owner->bytes; offset += PAGE_SIZE) {
        page_table_insert(base + (uintptr_t)offset, owner);
    }
}

static void unregister_pages(const page *owner) {
    uintptr_t base = (uintptr_t)owner;
    for (size_t offset = 0; offset < owner->bytes; offset += PAGE_SIZE) {
        page_table_remove(base + (uintptr_t)offset);
    }
}

/* Builds one page that holds blocks of `block_size` bytes. */
static page *page_new(size_t block_size, size_t bytes) {
    void *memory = aligned_alloc(PAGE_SIZE, bytes);
    if (memory == NULL) {
        return NULL;
    }
    memset(memory, 0, bytes);

    page *owner = memory;
    owner->bytes = bytes;
    owner->block_size = block_size;

    /* The header, then the two bitmaps, then the blocks. */
    size_t overhead = round_up(sizeof *owner, 16u);
    size_t available = bytes - overhead;
    /* Each block costs its size plus two bits, so solve for the count. */
    uint32_t count = (uint32_t)(available * 8u / (block_size * 8u + 2u));
    if (count == 0u) {
        free(memory);
        return NULL;
    }
    size_t words = (size_t)((count + 63u) / 64u);
    owner->alloc_bits = (uint64_t *)(void *)((uint8_t *)memory + overhead);
    owner->mark_bits = owner->alloc_bits + words;
    owner->blocks = (uint8_t *)(void *)(owner->mark_bits + words);
    /* The blocks must start aligned for the widest scalar. */
    size_t used = (size_t)(owner->blocks - (uint8_t *)memory);
    size_t aligned = round_up(used, 16u);
    owner->blocks = (uint8_t *)memory + aligned;
    while (aligned + (size_t)count * block_size > bytes) {
        count -= 1u;
    }
    owner->block_count = count;
    owner->used = 0;

    stats.heap_bytes += bytes;
    register_pages(owner);
    return owner;
}

static void page_free(page *owner) {
    stats.heap_bytes -= owner->bytes;
    unregister_pages(owner);
    free(owner);
}

/* Returns the size class for a request, or CLASS_COUNT for a large object. */
static size_t class_for(size_t bytes) {
    for (size_t index = 0; index < CLASS_COUNT; index += 1) {
        if (bytes <= SIZE_CLASSES[index]) {
            return index;
        }
    }
    return CLASS_COUNT;
}

/* Returns the index of a free block, or the block count when the page is full. */
static uint32_t first_free(page *owner) {
    for (uint32_t index = owner->cursor; index < owner->block_count; index += 1) {
        if (!bit_get(owner->alloc_bits, index)) {
            owner->cursor = index;
            return index;
        }
    }
    owner->cursor = owner->block_count;
    return owner->block_count;
}

/* -- The worklist -------------------------------------------------------- */

static void worklist_push(void *base) {
    if (worklist_length == worklist_capacity) {
        size_t capacity = worklist_capacity == 0 ? 256u : worklist_capacity * 2u;
        void **grown = realloc(worklist, capacity * sizeof *grown);
        if (grown == NULL) {
            abort();
        }
        worklist = grown;
        worklist_capacity = capacity;
    }
    worklist[worklist_length] = base;
    worklist_length += 1;
}

/* -- The collector interface --------------------------------------------- */

static void marksweep_init(const lark_gc_config *value) {
    configured_limit = value->heap_limit == 0 ? DEFAULT_LIMIT : value->heap_limit;
    heap_limit = configured_limit;
    memset(&stats, 0, sizeof stats);
}

static void free_list(page **head) {
    while (*head != NULL) {
        page *dead = *head;
        *head = dead->next;
        page_free(dead);
    }
}

static void marksweep_shutdown(void) {
    for (size_t index = 0; index < CLASS_COUNT; index += 1) {
        free_list(&pages[index]);
        /* The cursor named a page that `free_list` released. */
        alloc_cursor[index] = NULL;
    }
    free_list(&large_pages);
    free(page_table);
    page_table = NULL;
    page_table_capacity = 0;
    page_table_used = 0;
    free(worklist);
    worklist = NULL;
    worklist_capacity = 0;
    worklist_length = 0;
    memset(&stats, 0, sizeof stats);
}

static void *marksweep_base_of(void *interior) {
    page *owner = page_of(interior);
    if (owner == NULL) {
        return NULL;
    }
    const uint8_t *address = interior;
    if (address < owner->blocks) {
        return NULL;
    }
    size_t offset = (size_t)(address - owner->blocks);
    size_t span = (size_t)owner->block_count * owner->block_size;
    if (offset >= span) {
        return NULL;
    }
    uint32_t index = (uint32_t)(offset / owner->block_size);
    if (!bit_get(owner->alloc_bits, index)) {
        return NULL;
    }
    return owner->blocks + (size_t)index * owner->block_size + sizeof(lark_header);
}

/* Marks one candidate value, and queues it when it is a live object. */
static void mark_value(void *value, void **slot, void *ctx) {
    (void)slot;
    (void)ctx;
    if (value == NULL) {
        return;
    }
    void *base = marksweep_base_of(value);
    if (base == NULL) {
        return;
    }
    page *owner = page_of(base);
    if (owner == NULL) {
        return;
    }
    size_t offset = (size_t)((const uint8_t *)base - sizeof(lark_header) - owner->blocks);
    uint32_t index = (uint32_t)(offset / owner->block_size);
    if (bit_get(owner->mark_bits, index)) {
        return;
    }
    bit_set(owner->mark_bits, index);
    worklist_push(base);
}

/* Walks every managed field of every queued object. */
static void drain_worklist(void) {
    while (worklist_length > 0u) {
        worklist_length -= 1;
        uint8_t *base = worklist[worklist_length];
        const lark_header *header = LARK_HEADER(base);
        const lark_typeinfo *type = header->type;
        if (type == NULL || type->nptrs == 0u) {
            continue;
        }
        for (uintptr_t element = 0; element < header->count; element += 1) {
            uint8_t *item = base + (size_t)element * type->size;
            for (uint32_t field = 0; field < type->nptrs; field += 1) {
                void **slot = (void **)(void *)(item + type->ptr_offsets[field]);
                mark_value(*slot, slot, NULL);
            }
        }
    }
}

/* Clears every mark bit before a new mark phase. */
static void clear_marks(page *head) {
    for (page *owner = head; owner != NULL; owner = owner->next) {
        size_t words = (size_t)((owner->block_count + 63u) / 64u);
        memset(owner->mark_bits, 0, words * sizeof *owner->mark_bits);
    }
}

/* Frees every unmarked block, and reports what stayed alive. */
static void sweep_list(page **head) {
    page **link = head;
    while (*link != NULL) {
        page *owner = *link;
        for (uint32_t index = 0; index < owner->block_count; index += 1) {
            if (!bit_get(owner->alloc_bits, index)) {
                continue;
            }
            if (bit_get(owner->mark_bits, index)) {
                const uint8_t *block = owner->blocks + (size_t)index * owner->block_size;
                const lark_header *header = (const lark_header *)(const void *)block;
                stats.live_objects += 1;
                if (header->type != NULL) {
                    stats.live_bytes += header->type->size * (size_t)header->count;
                }
                continue;
            }
            bit_clear(owner->alloc_bits, index);
            owner->used -= 1;
        }
        owner->cursor = 0u;
        if (owner->used == 0u) {
            *link = owner->next;
            page_free(owner);
            continue;
        }
        link = &owner->next;
    }
}

static void marksweep_collect(void) {
    for (size_t index = 0; index < CLASS_COUNT; index += 1) {
        clear_marks(pages[index]);
    }
    clear_marks(large_pages);

    worklist_length = 0;
    lark_visit_roots(mark_value, NULL);
    drain_worklist();

    stats.live_objects = 0;
    stats.live_bytes = 0;
    for (size_t index = 0; index < CLASS_COUNT; index += 1) {
        /* A sweep frees blocks anywhere, and it can free the page that the
         * cursor names, so every cursor goes back to the head. */
        alloc_cursor[index] = NULL;
        sweep_list(&pages[index]);
    }
    sweep_list(&large_pages);
    stats.collections += 1;

    /* The next collection runs when the heap reaches a multiple of what it
     * holds after this sweep. Without this the limit never moves, and a live
     * set above it makes every later allocation collect.
     *
     * The factor applies to `heap_bytes`, which is what the trigger reads.
     * `live_bytes` counts payload alone, so a limit built from it would sit
     * below the heap size that the same objects need and collect at once. */
    size_t wanted = stats.heap_bytes * GROWTH_FACTOR;
    if (wanted < configured_limit) {
        wanted = configured_limit;
    }
    heap_limit = wanted;
}

static void *allocate_in(page *owner) {
    uint32_t index = first_free(owner);
    if (index >= owner->block_count) {
        return NULL;
    }
    bit_set(owner->alloc_bits, index);
    owner->used += 1;
    owner->cursor = index + 1u;
    return owner->blocks + (size_t)index * owner->block_size;
}

static void *marksweep_alloc(const lark_typeinfo *type, size_t count) {
    if (count == 0u) {
        count = 1u;
    }
    size_t payload = type->size * count;
    size_t need = sizeof(lark_header) + payload;

    /* A heap that grew past the limit collects before it grows again. */
    if (stats.heap_bytes > heap_limit) {
        lark_collect();
    }

    uint8_t *block = NULL;
    size_t klass = class_for(need);
    if (klass < CLASS_COUNT) {
        size_t block_size = SIZE_CLASSES[klass];
        page *start = alloc_cursor[klass] != NULL ? alloc_cursor[klass] : pages[klass];
        for (page *owner = start; owner != NULL; owner = owner->next) {
            block = allocate_in(owner);
            if (block != NULL) {
                alloc_cursor[klass] = owner;
                break;
            }
        }
        if (block == NULL) {
            page *owner = page_new(block_size, PAGE_SIZE);
            if (owner == NULL) {
                return NULL;
            }
            owner->next = pages[klass];
            pages[klass] = owner;
            /* The new page is the head, and the cursor points into the middle
             * of the list, so it moves back to the page with room. */
            alloc_cursor[klass] = owner;
            block = allocate_in(owner);
        }
    } else {
        size_t bytes = round_up(round_up(sizeof(page), 16u) + 16u + need, PAGE_SIZE);
        page *owner = page_new(need, bytes);
        if (owner == NULL) {
            return NULL;
        }
        owner->next = large_pages;
        large_pages = owner;
        block = allocate_in(owner);
    }
    if (block == NULL) {
        return NULL;
    }

    lark_header *header = (lark_header *)(void *)block;
    header->type = type;
    header->count = count;
    void *result = block + sizeof(lark_header);
    /* Rule O-5. A field with no designator is zero, and a managed field must
     * never hold garbage when a collection starts. */
    memset(result, 0, payload);

    stats.total_allocations += 1;
    return result;
}

static lark_gc_stats marksweep_statistics(void) {
    return stats;
}

static const lark_collector MARKSWEEP = {
    .name = "precise-marksweep",
    .caps = {
        .interior_pointers = true,   /* rule M-8 */
        .moving = false,
        .precise_heap = true,        /* the field map is mandatory */
        .precise_stack = true,       /* under shadow stack mode */
        .concurrent = false,
        .reclaims = true,
    },
    .init = marksweep_init,
    .shutdown = marksweep_shutdown,
    .alloc = marksweep_alloc,
    .collect = marksweep_collect,
    .base_of = marksweep_base_of,
    .statistics = marksweep_statistics,
};

const lark_collector *lark_gc_collector(void) {
    return &MARKSWEEP;
}
