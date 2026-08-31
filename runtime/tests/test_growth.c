/* How a collector sizes itself as the live set grows.
 *
 * Every one of these tests fails as a timeout or as wrong data when a
 * collector uses a fixed trigger or moves a space that holds live objects. A
 * correctness test with a small heap never reaches the case, so these tests
 * hold a live set well above the default size of every space.
 *
 * Test type T8 in docs/test-strategy.md.
 * covers: R-6, R-7, R-8 */

#include "lark_test.h"
#include "test_types.h"

/* Enough objects that every collector must grow past its default space. */
#define COUNT 40000u

static void *roots[1];

static void begin(void) {
    /* Not `test_start`, which asks for a 64 MB heap so that a test decides
     * when the collector runs. These tests are about the policy that decides
     * that, so they take the default size of every space. */
    lark_gc_config config = lark_gc_config_default();
    config.roots = LARK_ROOTS_SHADOW_STACK;
    lark_startup_with(config);
    roots[0] = NULL;
    lark_root_register(roots, 1);
}

static void end(void) {
    lark_root_remove(roots);
    lark_shutdown();
}

/* Builds a list of `count` nodes and returns it, with the head in `roots[0]`.
 *
 * The head stays in the root array through every allocation, so a collector
 * that moves objects rewrites it and the list stays whole. */
static void build(size_t count) {
    for (size_t index = 0; index < count; index += 1) {
        node *item = lark_alloc(&NODE_TYPE);
        if (item == NULL) {
            return;
        }
        item->left = roots[0];
        item->value = (int)index;
        roots[0] = item;
    }
}

/* Returns the length of the list, and -1 when a value is wrong.
 *
 * The values run down from `count - 1`, so a wrong value proves that a
 * collection moved an object without rewriting the pointer that named it. */
static long walk(size_t count) {
    long seen = 0;
    int expected = (int)count - 1;
    for (node *item = roots[0]; item != NULL; item = item->left) {
        if (item->value != expected) {
            return -1;
        }
        expected -= 1;
        seen += 1;
    }
    return seen;
}

/* Rule R-6. A live set above the default trigger must not collect on every
 * allocation. A collector that does never finishes this test. */
void test_a_growing_live_set_collects_a_bounded_number_of_times(void);
void test_a_growing_live_set_collects_a_bounded_number_of_times(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims, "this collector never collects");
    begin();

    build(COUNT);
    REQUIRE(roots[0] != NULL);
    CHECK(walk(COUNT) == (long)COUNT);

    /* Each collection must at least double the space it manages, so the count
     * grows with the logarithm of the heap. The bound is loose on purpose: it
     * catches a fixed trigger, which collects tens of thousands of times, and
     * it does not pin any collector to one policy. */
    lark_gc_stats stats = lark_gc_statistics();
    CHECK(stats.collections < 200u);

    end();
}

/* Rule R-7. A collector that grows a space must rewrite every pointer into it.
 * A raw copy of a space that holds live objects leaves every pointer naming an
 * address that moved, and the walk then reads the wrong value. */
void test_a_grown_heap_keeps_every_value(void);
void test_a_grown_heap_keeps_every_value(void) {
    begin();

    build(COUNT);
    REQUIRE(roots[0] != NULL);
    /* -1 means a value did not match, which is the shape that a space moved
     * without a rewrite produces. */
    CHECK(walk(COUNT) != -1);
    CHECK(walk(COUNT) == (long)COUNT);

    /* A collection after the growth must keep the list whole as well. */
    lark_collect();
    CHECK(walk(COUNT) == (long)COUNT);

    end();
}

/* Rule R-6. A heap that drops its live set collects less often afterward,
 * because the trigger follows what the last collection left. */
void test_a_dropped_live_set_lowers_the_trigger(void);
void test_a_dropped_live_set_lowers_the_trigger(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims, "this collector never collects");
    begin();

    build(COUNT);
    roots[0] = NULL;
    lark_collect();

    lark_gc_stats after = lark_gc_statistics();
    CHECK(after.live_objects == 0u);

    /* The same work again must cost about the same number of collections, and
     * not one per allocation. */
    size_t before = lark_gc_statistics().collections;
    build(COUNT);
    CHECK(walk(COUNT) == (long)COUNT);
    CHECK(lark_gc_statistics().collections - before < 200u);

    end();
}
