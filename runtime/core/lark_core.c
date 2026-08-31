/* The runtime core: threads, the shadow stack, safepoints, and transitions.
 *
 * Chapter 03 of docs/spec gives every rule that this file implements. */

#if defined(__linux__) && !defined(_GNU_SOURCE)
#define _GNU_SOURCE
#endif

#include <pthread.h>
#include <stdlib.h>
#include <string.h>

#include "lark_internal.h"
#include "lark_rt.h"

/* What a thread is doing. A collection can start when every thread except the
 * collector is SAFE or PARKED. */
enum thread_state {
    STATE_RUNNING = 0,   /* inside Lark code, holding managed pointers */
    STATE_SAFE = 1,      /* inside a foreign call, rule M-19 */
    STATE_PARKED = 2     /* stopped at a safepoint, rule M-26 */
};

/* One registered thread. */
typedef struct thread_entry {
    struct thread_entry *next;
    lark_frame_hdr *frames;      /* the shadow stack head, rule M-25 */
    void *stack_base;            /* the high end of the machine stack */
    void *stack_top;             /* the low end, recorded when the thread stops */
    jmp_buf registers;           /* a spill area for a conservative scan */
    int state;
    /* How many safe regions the thread is inside.
     *
     * One safe call can hold another, as in `f(g())` where both are foreign.
     * A count keeps the thread safe until the outermost call returns. */
    unsigned safe_depth;
} thread_entry;

/* One registered array of global roots. See rule M-9. */
typedef struct root_entry {
    struct root_entry *next;
    void **slots;
    size_t count;
} root_entry;

volatile int lark_gc_request = 0;

static pthread_mutex_t world_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t world_cond = PTHREAD_COND_INITIALIZER;

static thread_entry *threads;
static size_t thread_total;
static root_entry *roots;
static bool running;
static bool collecting;
static lark_gc_config config;

static _Thread_local thread_entry *current;

/* Returns the high end of the calling thread's machine stack.
 *
 * A conservative scan reads from where the thread stands up to this address.
 * The platform knows the real end. The fallback uses the caller's frame, which
 * misses a root that sits above it. */
static void *stack_base_of_current_thread(void *fallback) {
#if defined(__APPLE__)
    (void)fallback;
    return pthread_get_stackaddr_np(pthread_self());
#elif defined(__linux__)
    pthread_attr_t attributes;
    if (pthread_getattr_np(pthread_self(), &attributes) != 0) {
        return fallback;
    }
    void *low = NULL;
    size_t size = 0;
    int failed = pthread_attr_getstack(&attributes, &low, &size);
    pthread_attr_destroy(&attributes);
    if (failed != 0 || low == NULL) {
        return fallback;
    }
    return (void *)((char *)low + size);
#else
    return fallback;
#endif
}

lark_gc_config lark_gc_config_default(void) {
    lark_gc_config value;
    value.roots = LARK_ROOTS_SHADOW_STACK;
    value.torture = false;
    value.heap_limit = 0;
    return value;
}

lark_roots lark_root_mode(void) {
    return config.roots;
}

const lark_gc_config *lark_runtime_config(void) {
    return &config;
}

bool lark_is_running(void) {
    return running;
}

void lark_startup(void) {
    lark_startup_with(lark_gc_config_default());
}

void lark_startup_with(lark_gc_config value) {
    if (running) {
        return;
    }
    /* Rule F-4. The variable turns torture mode on for a binary that the build
     * did not configure for it. */
    const char *torture = getenv("LARK_GC_TORTURE");
    if (torture != NULL && strcmp(torture, "0") != 0) {
        value.torture = true;
    }
    config = value;
    /* Rule I-4 step 1. The collector starts before anything else. */
    lark_gc_collector()->init(&config);
    running = true;
    /* Rule I-4 step 2. The calling thread registers itself. */
    lark_thread_attach();
}

void lark_shutdown(void) {
    if (!running) {
        return;
    }
    lark_thread_detach();

    pthread_mutex_lock(&world_lock);
    while (roots != NULL) {
        root_entry *dead = roots;
        roots = roots->next;
        free(dead);
    }
    pthread_mutex_unlock(&world_lock);

    lark_gc_collector()->shutdown();
    running = false;
}

/* -- Threads ------------------------------------------------------------- */

void lark_thread_attach(void) {
    if (current != NULL) {
        return;
    }
    thread_entry *entry = calloc(1, sizeof *entry);
    if (entry == NULL) {
        abort();
    }
    void *here = NULL;
    entry->state = STATE_RUNNING;
    entry->stack_base = stack_base_of_current_thread((void *)&here);
    entry->stack_top = (void *)&here;

    pthread_mutex_lock(&world_lock);
    entry->next = threads;
    threads = entry;
    thread_total += 1;
    pthread_mutex_unlock(&world_lock);

    current = entry;
}

void lark_thread_detach(void) {
    thread_entry *entry = current;
    if (entry == NULL) {
        return;
    }
    current = NULL;

    pthread_mutex_lock(&world_lock);
    thread_entry **link = &threads;
    while (*link != NULL && *link != entry) {
        link = &(*link)->next;
    }
    if (*link == entry) {
        *link = entry->next;
        thread_total -= 1;
    }
    /* A collector that waits for this thread must stop waiting. */
    pthread_cond_broadcast(&world_cond);
    pthread_mutex_unlock(&world_lock);

    free(entry);
}

size_t lark_thread_count(void) {
    pthread_mutex_lock(&world_lock);
    size_t count = thread_total;
    pthread_mutex_unlock(&world_lock);
    return count;
}

/* -- The shadow stack ---------------------------------------------------- */

void lark_frame_push(lark_frame_hdr *frame) {
    thread_entry *entry = current;
    if (entry == NULL) {
        return;
    }
    frame->prev = entry->frames;
    entry->frames = frame;
}

void lark_frame_pop(lark_frame_hdr *frame) {
    thread_entry *entry = current;
    if (entry == NULL) {
        return;
    }
    entry->frames = frame->prev;
}

lark_frame_hdr *lark_frame_top(void) {
    return current == NULL ? NULL : current->frames;
}

void lark_frame_restore(lark_frame_hdr *frame) {
    if (current != NULL) {
        current->frames = frame;
    }
}

_Noreturn void lark_longjmp(lark_jmp_buf *buffer, int value) {
    /* Rule M-15. A jump skips every frame pop between here and the target. */
    lark_frame_restore(buffer->top);
    longjmp(buffer->jb, value == 0 ? 1 : value);
}

/* -- Safepoints and the stop the world protocol -------------------------- */

/* Records where the calling thread stands, for a conservative scan. */
static void record_stack(thread_entry *entry) {
    void *here = NULL;
    entry->stack_top = (void *)&here;
    /* The buffer spills the callee saved registers onto the stack, so the
     * scan below finds a pointer that lives only in a register. */
    (void)setjmp(entry->registers);
}

/* Reports whether every thread except the collector stopped.
 *
 * The caller holds `world_lock`. */
static bool world_is_stopped(const thread_entry *collector) {
    for (const thread_entry *entry = threads; entry != NULL; entry = entry->next) {
        if (entry == collector) {
            continue;
        }
        if (entry->state == STATE_RUNNING) {
            return false;
        }
    }
    return true;
}

void lark_safepoint(void) {
    thread_entry *entry = current;
    if (entry == NULL) {
        return;
    }
    record_stack(entry);

    pthread_mutex_lock(&world_lock);
    while (lark_gc_request != 0) {
        entry->state = STATE_PARKED;
        pthread_cond_broadcast(&world_cond);
        pthread_cond_wait(&world_cond, &world_lock);
    }
    entry->state = STATE_RUNNING;
    pthread_mutex_unlock(&world_lock);
}

void lark_enter_safe(void) {
    thread_entry *entry = current;
    if (entry == NULL) {
        return;
    }
    entry->safe_depth += 1u;
    if (entry->safe_depth > 1u) {
        /* The thread is already safe, and an inner call changes nothing. */
        return;
    }
    record_stack(entry);

    pthread_mutex_lock(&world_lock);
    entry->state = STATE_SAFE;
    pthread_cond_broadcast(&world_cond);
    pthread_mutex_unlock(&world_lock);
}

void lark_leave_safe(void) {
    thread_entry *entry = current;
    if (entry == NULL || entry->safe_depth == 0u) {
        return;
    }
    entry->safe_depth -= 1u;
    if (entry->safe_depth > 0u) {
        /* An outer call still holds the thread in the safe state. */
        return;
    }
    pthread_mutex_lock(&world_lock);
    while (lark_gc_request != 0) {
        pthread_cond_wait(&world_cond, &world_lock);
    }
    entry->state = STATE_RUNNING;
    pthread_mutex_unlock(&world_lock);
}

void lark_collect(void) {
    thread_entry *entry = current;
    if (entry == NULL || !running) {
        return;
    }
    record_stack(entry);

    /* Rule M-26. The thread parks itself before it asks for the lock.
     *
     * A collector waits until every other thread is parked. A thread that
     * blocks on the lock while it still says it is running would hold that
     * wait open for ever, and the two would deadlock. The state is written
     * here rather than after the lock, so the window does not exist.
     *
     * The write is not under the lock, and the reader holds the lock. Both
     * sides read and write one `int`, so a torn value is impossible, and a
     * stale read only delays the collector by one wakeup. */
    entry->state = STATE_PARKED;

    pthread_mutex_lock(&world_lock);
    /* Another thread already collects. Wait for it, and take its result. */
    while (collecting) {
        entry->state = STATE_PARKED;
        pthread_cond_broadcast(&world_cond);
        pthread_cond_wait(&world_cond, &world_lock);
    }
    entry->state = STATE_RUNNING;

    collecting = true;
    lark_gc_request = 1;
    entry->state = STATE_PARKED;
    pthread_cond_broadcast(&world_cond);
    while (!world_is_stopped(entry)) {
        pthread_cond_wait(&world_cond, &world_lock);
    }

    lark_gc_collector()->collect();

    lark_gc_request = 0;
    collecting = false;
    entry->state = STATE_RUNNING;
    pthread_cond_broadcast(&world_cond);
    pthread_mutex_unlock(&world_lock);
}

/* -- Roots --------------------------------------------------------------- */

void lark_root_register(void **slots, size_t count) {
    root_entry *entry = calloc(1, sizeof *entry);
    if (entry == NULL) {
        abort();
    }
    entry->slots = slots;
    entry->count = count;

    pthread_mutex_lock(&world_lock);
    entry->next = roots;
    roots = entry;
    pthread_mutex_unlock(&world_lock);
}

void lark_root_remove(void **slots) {
    pthread_mutex_lock(&world_lock);
    root_entry **link = &roots;
    while (*link != NULL) {
        if ((*link)->slots == slots) {
            root_entry *dead = *link;
            *link = dead->next;
            free(dead);
            break;
        }
        link = &(*link)->next;
    }
    pthread_mutex_unlock(&world_lock);
}

/* Scans a range of machine words and reports each as a candidate root. */
static void scan_words(void *low, void *high, lark_root_visit visit, void *ctx) {
    if (low == NULL || high == NULL || low >= high) {
        return;
    }
    /* A pointer is aligned, so the scan steps by one word. */
    void **word = (void **)low;
    void **end = (void **)high;
    for (; word < end; word += 1) {
        visit(*word, NULL, ctx);
    }
}

void lark_visit_roots(lark_root_visit visit, void *ctx) {
    for (const root_entry *entry = roots; entry != NULL; entry = entry->next) {
        for (size_t index = 0; index < entry->count; index += 1) {
            visit(entry->slots[index], &entry->slots[index], ctx);
        }
    }

    for (const thread_entry *entry = threads; entry != NULL; entry = entry->next) {
        if (config.roots == LARK_ROOTS_SHADOW_STACK) {
            /* Rule M-10. Only a function with a managed local has a frame. */
            for (const lark_frame_hdr *frame = entry->frames; frame != NULL;
                 frame = frame->prev) {
                /* A slot holds the address of a local, so the value needs one
                 * more load. The local keeps its own name in the emitted C. */
                for (uint32_t index = 0; index < frame->nslots; index += 1) {
                    void **slot = frame->slots[index];
                    if (slot != NULL) {
                        visit(*slot, slot, ctx);
                    }
                }
                /* Rule M-27. A temporary slot holds the value itself. */
                for (uint32_t index = 0; index < frame->ntemps; index += 1) {
                    visit(frame->temps[index], &frame->temps[index], ctx);
                }
            }
        } else {
            /* Rule M-13. The scan reads every word of the machine stack. */
            scan_words(entry->stack_top, entry->stack_base, visit, ctx);
            scan_words((void *)&entry->registers,
                       (void *)((const char *)&entry->registers + sizeof entry->registers),
                       visit, ctx);
        }
    }
}

/* -- Allocation ---------------------------------------------------------- */

void *lark_alloc(const lark_typeinfo *type) {
    return lark_alloc_array(type, 1);
}

void *lark_alloc_array(const lark_typeinfo *type, size_t count) {
    if (type == NULL) {
        return NULL;
    }
    /* Rule M-16. A poll goes before every managed allocation. */
    LARK_POLL();
    if (config.torture) {
        /* Rule F-3. Every safepoint runs a full collection. */
        lark_collect();
    }
    return lark_gc_collector()->alloc(type, count);
}

void *lark_new(const lark_typeinfo *type, const void *initial) {
    void *result = lark_alloc(type);
    if (result != NULL && initial != NULL) {
        memcpy(result, initial, type->size);
    }
    return result;
}

const void *lark_itable_find(const lark_typeinfo *type, const void *iface) {
    if (type == NULL || iface == NULL) {
        return NULL;
    }
    for (uint32_t index = 0; index < type->nitables; index += 1) {
        if (type->itables[index].iface == iface) {
            return type->itables[index].vtable;
        }
    }
    return NULL;
}

const lark_typeinfo lark_bytes_type = {
    "bytes", 1u, 1u, 0u, NULL, 0u, NULL,
};

void *lark_base_of(void *interior) {
    return lark_gc_collector()->base_of(interior);
}

lark_gc_stats lark_gc_statistics(void) {
    return lark_gc_collector()->statistics();
}

lark_gc_caps lark_gc_capabilities(void) {
    return lark_gc_collector()->caps;
}

const char *lark_gc_name(void) {
    return lark_gc_collector()->name;
}

void lark_write_barrier(void **slot, void *value) {
    const lark_collector *collector = lark_gc_collector();
    if (collector->write_barrier != NULL) {
        collector->write_barrier(slot, value);
        return;
    }
    /* Rule R-2. A collector with no barrier performs the store and returns. */
    *slot = value;
}
