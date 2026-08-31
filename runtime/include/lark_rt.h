/* The Lark runtime.
 *
 * Chapter 10 of docs/spec defines this interface. The runtime has two layers.
 * The core holds threads, the shadow stack, safepoints, and the state machine
 * for a foreign call. The collector reclaims managed memory behind a fixed
 * interface, and a program links exactly one.
 *
 * A program that uses no managed type links neither layer. */

#ifndef LARK_RT_H
#define LARK_RT_H

#include <setjmp.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* -- Type information ---------------------------------------------------- */

/* One entry of a type's interface table.
 *
 * `iface` is a unique address per interface, so two modules that name the same
 * interface still compare equal. `vtable` holds the method table for the pair
 * of interface and type. See rules O-13 and O-23. */
typedef struct lark_itable_ent {
    const void *iface;
    const void *vtable;
} lark_itable_ent;

/* One managed type. Rule M-5 makes the field map mandatory, so heap tracing is
 * precise under every collector. */
typedef struct lark_typeinfo {
    const char *name;
    size_t size;                       /* payload size in bytes */
    uint32_t align;
    uint32_t nptrs;                    /* number of managed fields */
    const uint32_t *ptr_offsets;       /* byte offset of each managed field */
    uint32_t nitables;
    const lark_itable_ent *itables;
} lark_typeinfo;

/* The header that sits before every managed object. See rule M-4.
 *
 * A managed pointer refers to the payload, not to the header, so the payload
 * keeps C layout and rule O-3 holds. */
typedef struct lark_header {
    const lark_typeinfo *type;
    uintptr_t count;                   /* element count, 1 for one object */
} lark_header;

/* Returns the header of a managed object. */
#define LARK_HEADER(p) (((lark_header *)(p)) - 1)

/* -- Collector capabilities ---------------------------------------------- */

/* What one collector supports. The transpiler reads this and enforces the
 * matching source rules. See rule R-1. */
typedef struct lark_gc_caps {
    bool interior_pointers;            /* rule M-8 */
    bool moving;
    bool precise_heap;                 /* always true, the field map is mandatory */
    bool precise_stack;                /* shadow stack mode only */
    bool concurrent;
    bool reclaims;                     /* a collection frees what nothing reaches */
    bool write_barrier;                /* a store of a managed pointer is recorded */
} lark_gc_caps;

/* How the runtime finds the roots on a stack. */
typedef enum lark_roots {
    LARK_ROOTS_SHADOW_STACK = 0,       /* precise, rule M-10 */
    LARK_ROOTS_CONSERVATIVE = 1        /* scans the machine stack, rule M-13 */
} lark_roots;

/* The settings that a program gives the runtime at startup. */
typedef struct lark_gc_config {
    lark_roots roots;
    bool torture;                      /* collect at every safepoint, rule F-3 */
    size_t heap_limit;                 /* bytes before a collection, 0 for the default */
} lark_gc_config;

/* Returns the default configuration. */
lark_gc_config lark_gc_config_default(void);

/* -- Startup and shutdown ------------------------------------------------ */

/* Starts the collector and registers the calling thread. See rule I-4. */
void lark_startup(void);

/* Starts the runtime with settings other than the defaults. */
void lark_startup_with(lark_gc_config config);

/* Releases every heap page. The program must hold no managed pointer after. */
void lark_shutdown(void);

/* Reports whether the runtime is running. */
bool lark_is_running(void);

/* -- Threads ------------------------------------------------------------- */

/* Registers the calling thread. Rule M-24 requires this before the thread
 * touches managed memory. */
void lark_thread_attach(void);

/* Unregisters the calling thread. */
void lark_thread_detach(void);

/* Returns the number of registered threads. */
size_t lark_thread_count(void);

/* -- The shadow stack ---------------------------------------------------- */

/* One frame of the shadow stack.
 *
 * Rule M-10 emits a frame only for a function that holds a managed value, so
 * every other function costs nothing.
 *
 * `slots` holds the address of each managed local, so the local keeps the name
 * that the programmer wrote. `temps` holds the value of each managed temporary,
 * because a fresh object belongs to no local yet. See rule M-27. */
typedef struct lark_frame_hdr {
    struct lark_frame_hdr *prev;
    void ***slots;
    void **temps;
    uint32_t nslots;
    uint32_t ntemps;
} lark_frame_hdr;

/* Pushes a frame. Rule M-11 raises the slot count as each local appears. */
void lark_frame_push(lark_frame_hdr *frame);

/* Pops a frame. Rule M-12 requires this on every exit path. */
void lark_frame_pop(lark_frame_hdr *frame);

/* Returns the innermost frame of the calling thread. */
lark_frame_hdr *lark_frame_top(void);

/* Sets the innermost frame of the calling thread. `lark_longjmp` uses this. */
void lark_frame_restore(lark_frame_hdr *frame);

/* -- Safepoints ---------------------------------------------------------- */

/* Set when a thread asks the world to stop. See rule M-17. */
extern volatile int lark_gc_request;

/* Parks the calling thread until the collection finishes. */
void lark_safepoint(void);

/* The poll that rule M-16 places at an allocation and at a loop back edge. */
#define LARK_POLL()                        \
    do {                                   \
        if (lark_gc_request) {              \
            lark_safepoint();              \
        }                                  \
    } while (0)

/* -- Foreign calls ------------------------------------------------------- */

/* Enters the safe state before a `gc_safe` call. See rule M-19. */
void lark_enter_safe(void);

/* Leaves the safe state after a `gc_safe` call. */
void lark_leave_safe(void);

/* -- Non local jumps ----------------------------------------------------- */

/* A jump buffer that also carries the shadow stack head. See rule M-15. */
typedef struct lark_jmp_buf {
    jmp_buf jb;
    lark_frame_hdr *top;
} lark_jmp_buf;

/* Saves the shadow stack head with the jump target. */
#define lark_setjmp(b) (((b)->top = lark_frame_top()), setjmp((b)->jb))

/* Restores the shadow stack head, then jumps. */
_Noreturn void lark_longjmp(lark_jmp_buf *buffer, int value);

/* -- Global roots -------------------------------------------------------- */

/* Registers an array of managed slots as roots. A `@global` block calls this
 * from its initializer. See rule M-9. */
void lark_root_register(void **slots, size_t count);

/* Removes an array of managed slots from the roots. */
void lark_root_remove(void **slots);

/* -- Allocation ---------------------------------------------------------- */

/* Allocates one managed object. See rule O-4. */
void *lark_alloc(const lark_typeinfo *type);

/* Allocates a managed array of `count` elements. See rule O-6. */
void *lark_alloc_array(const lark_typeinfo *type, size_t count);

/* Allocates one object and copies the initial value into it.
 *
 * `new T { ... }` becomes one call to this, with a compound literal for the
 * initial value. The form stays an expression, which C needs. */
void *lark_new(const lark_typeinfo *type, const void *initial);

/* A descriptor for raw bytes, with no managed field.
 *
 * `new char[256]` and every other array of a plain element use it. */
extern const lark_typeinfo lark_bytes_type;

/* Runs a full collection now. */
void lark_collect(void);

/* Returns the method table of a type for an interface, or NULL.
 *
 * A checked cast from an interface value back to a concrete type uses this.
 * See rule O-23. */
const void *lark_itable_find(const lark_typeinfo *type, const void *iface);

/* Returns the payload base for any address inside a managed object.
 *
 * Rule M-7 makes this constant time, so an interior pointer works. Returns
 * NULL for an address outside the managed heap. */
void *lark_base_of(void *interior);

/* -- Statistics ---------------------------------------------------------- */

/* What the collector has done so far. Tests read this. */
typedef struct lark_gc_stats {
    size_t live_objects;
    size_t live_bytes;
    size_t total_allocations;
    size_t collections;
    size_t heap_bytes;
} lark_gc_stats;

/* Returns the current statistics. */
lark_gc_stats lark_gc_statistics(void);

/* Returns the capabilities of the linked collector. */
lark_gc_caps lark_gc_capabilities(void);

/* Returns the name of the linked collector. */
const char *lark_gc_name(void);

/* Records a store of a managed pointer into a managed object.
 *
 * Rule R-2. `slot` is the address of the field, and `value` is what goes in
 * it. The store happens here, so the caller writes nothing of its own. A
 * collector that needs no barrier performs the store and returns.
 *
 * The transpiler emits a call only when the capability says the collector
 * needs one. Every other build stores directly. */
void lark_write_barrier(void **slot, void *value);

#ifdef __cplusplus
}
#endif

#endif /* LARK_RT_H */
