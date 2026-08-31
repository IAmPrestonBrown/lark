/* Internal state that the core shares with the collector.
 *
 * A program never includes this file. It is the seam between the two layers
 * that chapter 10 section 1 describes. */

#ifndef LARK_INTERNAL_H
#define LARK_INTERNAL_H

#include "lark_rt.h"

/* Called for every root that the core finds.
 *
 * `slot` is the address of the root when the core knows it exactly, and NULL
 * when a conservative scan produced the value. A moving collector needs the
 * slot. A non moving collector needs only the value. */
typedef void (*lark_root_visit)(void *value, void **slot, void *ctx);

/* Visits every root: the global arrays and each thread's stack.
 *
 * The caller must hold the world still. `lark_collect` does that. */
void lark_visit_roots(lark_root_visit visit, void *ctx);

/* Returns the root mechanism that the program configured. */
lark_roots lark_root_mode(void);

/* Returns the configuration that the program gave at startup. */
const lark_gc_config *lark_runtime_config(void);

/* -- The collector interface --------------------------------------------- */

/* One collector. A program links exactly one. See chapter 10 section 3. */
typedef struct lark_collector {
    const char *name;
    lark_gc_caps caps;

    void (*init)(const lark_gc_config *config);
    void (*shutdown)(void);

    void *(*alloc)(const lark_typeinfo *type, size_t count);
    void (*collect)(void);

    void *(*base_of)(void *interior);      /* rule M-7 */
    lark_gc_stats (*statistics)(void);

    /* Records a store of a managed pointer into a managed object.
     *
     * Rule R-2. A collector that needs no barrier leaves this NULL, and the
     * transpiler then emits a plain store. A generational collector needs it,
     * because a pointer from an old object to a young one is a root that a
     * minor collection cannot find any other way. */
    void (*write_barrier)(void **slot, void *value);
} lark_collector;

/* Returns the collector that the program linked. */
const lark_collector *lark_gc_collector(void);

#endif /* LARK_INTERNAL_H */
