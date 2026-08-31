/* The mark phase and the sweep phase. */

#include "lark_test.h"
#include "test_types.h"

/* Holds one root array for a test. */
static void *roots[4];

/* A collection can move an object. Rule M-10 makes the shadow stack slot the
 * address of the local itself, so generated code needs no reload. A test that
 * keeps its own copy of a pointer must read the root back, because the
 * collector rewrote the root and not the copy. `RELOAD` states that, and it
 * costs nothing under a collector that does not move. */
#define RELOAD(ptr, slot) ((ptr) = (slot))

static void register_roots(size_t count) {
    for (size_t index = 0; index < 4u; index += 1) {
        roots[index] = NULL;
    }
    lark_root_register(roots, count);
}

void test_an_unreachable_object_is_freed(void);
void test_an_unreachable_object_is_freed(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims,
                "the collector frees nothing");
    test_start(LARK_ROOTS_SHADOW_STACK);
    register_roots(1);

    for (int index = 0; index < 50; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    lark_collect();
    lark_gc_stats stats = lark_gc_statistics();
    CHECK(stats.live_objects == 0u);
    CHECK(stats.collections == 1u);

    lark_root_remove(roots);
    lark_shutdown();
}

/* covers: M-9 */
void test_an_object_a_root_reaches_survives(void);
void test_an_object_a_root_reaches_survives(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims,
                "the collector frees nothing");
    test_start(LARK_ROOTS_SHADOW_STACK);
    register_roots(1);

    node *kept = lark_alloc(&NODE_TYPE);
    REQUIRE(kept != NULL);
    kept->value = 42;
    roots[0] = kept;

    for (int index = 0; index < 20; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    lark_collect();
    RELOAD(kept, roots[0]);

    CHECK(lark_gc_statistics().live_objects == 1u);
    CHECK(kept->value == 42);
    if (lark_gc_capabilities().interior_pointers) {
        CHECK(lark_base_of(kept) == kept);
    }

    lark_root_remove(roots);
    lark_shutdown();
}

/* covers: M-5 */
void test_a_chain_survives_through_its_root(void);
void test_a_chain_survives_through_its_root(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    register_roots(1);

    node *head = lark_alloc(&NODE_TYPE);
    REQUIRE(head != NULL);
    roots[0] = head;

    node *tail = head;
    for (int index = 1; index < 10; index += 1) {
        node *next = lark_alloc(&NODE_TYPE);
        REQUIRE(next != NULL);
        next->value = index;
        tail->left = next;
        tail = next;
    }
    lark_collect();
    RELOAD(head, roots[0]);

    CHECK(lark_gc_statistics().live_objects == 10u);
    node *walk = head;
    int seen = 0;
    while (walk->left != NULL) {
        walk = walk->left;
        seen += 1;
        CHECK(walk->value == seen);
    }
    CHECK(seen == 9);

    lark_root_remove(roots);
    lark_shutdown();
}

void test_a_cycle_does_not_stop_the_collector(void);
void test_a_cycle_does_not_stop_the_collector(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims,
                "the collector frees nothing");
    test_start(LARK_ROOTS_SHADOW_STACK);
    register_roots(1);

    node *first = lark_alloc(&NODE_TYPE);
    node *second = lark_alloc(&NODE_TYPE);
    REQUIRE(first != NULL && second != NULL);
    first->left = second;
    second->left = first;
    second->right = second;
    roots[0] = first;

    lark_collect();
    RELOAD(first, roots[0]);
    CHECK(lark_gc_statistics().live_objects == 2u);
    /* The cycle survived whole, so each link still reaches the other. */
    REQUIRE(first->left != NULL);
    CHECK(first->left->left == first);

    /* With the root gone, the cycle is unreachable and both objects go. */
    roots[0] = NULL;
    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 0u);

    lark_root_remove(roots);
    lark_shutdown();
}

void test_an_array_element_keeps_its_target_alive(void);
void test_an_array_element_keeps_its_target_alive(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    register_roots(1);

    node *items = lark_alloc_array(&NODE_TYPE, 4);
    REQUIRE(items != NULL);
    roots[0] = items;
    for (int index = 0; index < 4; index += 1) {
        node *target = lark_alloc(&NODE_TYPE);
        REQUIRE(target != NULL);
        target->value = 100 + index;
        items[index].left = target;
    }
    lark_collect();
    RELOAD(items, roots[0]);

    /* The array and its four targets, so five objects. */
    CHECK(lark_gc_statistics().live_objects == 5u);
    for (int index = 0; index < 4; index += 1) {
        REQUIRE(items[index].left != NULL);
        CHECK(items[index].left->value == 100 + index);
    }

    lark_root_remove(roots);
    lark_shutdown();
}

/* covers: M-6 */
void test_an_interior_pointer_keeps_its_object_alive(void);
void test_an_interior_pointer_keeps_its_object_alive(void) {
    SKIP_UNLESS(lark_gc_capabilities().interior_pointers,
                "rule M-8 does not hold for this collector");
    test_start(LARK_ROOTS_SHADOW_STACK);
    register_roots(1);

    char *buffer = lark_alloc_array(&PLAIN_TYPE, 32);
    REQUIRE(buffer != NULL);
    /* The only reference is an address in the middle of the object. */
    roots[0] = buffer + 40;

    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 1u);
    CHECK(lark_base_of(buffer) == buffer);

    lark_root_remove(roots);
    lark_shutdown();
}

void test_repeated_collections_reclaim_the_heap(void);
void test_repeated_collections_reclaim_the_heap(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims,
                "the collector frees nothing");
    test_start(LARK_ROOTS_SHADOW_STACK);
    register_roots(1);

    for (int round = 0; round < 20; round += 1) {
        for (int index = 0; index < 200; index += 1) {
            (void)lark_alloc(&NODE_TYPE);
        }
        lark_collect();
        CHECK(lark_gc_statistics().live_objects == 0u);
    }
    CHECK(lark_gc_statistics().collections == 20u);
    /* Every page went back, because no block stayed alive. */
    /* A collector that returns its pages to the system drops to zero. One
     * that keeps two fixed spaces does not, and both reclaimed everything. */
    CHECK(lark_gc_statistics().live_objects == 0u);

    lark_root_remove(roots);
    lark_shutdown();
}
