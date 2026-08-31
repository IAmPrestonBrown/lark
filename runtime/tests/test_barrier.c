/* The write barrier, and the shape it exists for.
 *
 * A generational collector walks the nursery and not the old generation, so a
 * pointer from an old object to a young one is a root that nothing else
 * finds. Without the barrier the young object looks unreachable and goes, and
 * the old object then holds an address that no longer names it.
 *
 * That is the shape these tests build. A collector with no barrier passes them
 * too, because the store goes through the same call and rule R-2 makes it a
 * plain store there.
 *
 * Test type T8 in docs/test-strategy.md.
 * covers: R-2 */

#include <string.h>

#include "lark_test.h"
#include "test_types.h"

static void *roots[4];

static void begin(size_t count) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    memset(roots, 0, sizeof roots);
    lark_root_register(roots, count);
}

static void end(void) {
    lark_root_remove(roots);
    lark_shutdown();
}

/* Rule R-2. The barrier performs the store, so a caller writes nothing else. */
void test_the_barrier_performs_the_store(void);
void test_the_barrier_performs_the_store(void) {
    begin(2);

    node *holder = lark_alloc(&NODE_TYPE);
    node *target = lark_alloc(&NODE_TYPE);
    REQUIRE(holder != NULL && target != NULL);
    target->value = 11;
    roots[0] = holder;
    roots[1] = target;

    lark_write_barrier((void **)&((node *)roots[0])->left, roots[1]);

    REQUIRE(((node *)roots[0])->left != NULL);
    CHECK(((node *)roots[0])->left->value == 11);
    end();
}

/* A store of NULL through the barrier clears the field. */
void test_the_barrier_stores_a_null(void);
void test_the_barrier_stores_a_null(void) {
    begin(1);

    node *holder = lark_alloc(&NODE_TYPE);
    node *target = lark_alloc(&NODE_TYPE);
    REQUIRE(holder != NULL && target != NULL);
    holder->left = target;
    roots[0] = holder;

    lark_write_barrier((void **)&((node *)roots[0])->left, NULL);
    CHECK(((node *)roots[0])->left == NULL);
    end();
}

/* An old object that points at a young one keeps it alive.
 *
 * This is the shape that the barrier exists for. The holder is old, because a
 * collection ran after it was made. The target is young, because it was made
 * after that collection. A minor collection walks the nursery alone, so the
 * pointer in the old object is the only thing that reaches the target.
 * covers: R-2, M-9 */
void test_an_old_object_keeps_a_young_one_alive(void);
void test_an_old_object_keeps_a_young_one_alive(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims, "the collector frees nothing");
    begin(1);

    /* The holder becomes old, because a collection runs while it is rooted. */
    node *holder = lark_alloc(&NODE_TYPE);
    REQUIRE(holder != NULL);
    holder->value = 1;
    roots[0] = holder;
    lark_collect();

    /* The target is young. Only the field of the old holder names it. */
    node *target = lark_alloc(&NODE_TYPE);
    REQUIRE(target != NULL);
    target->value = 22;
    lark_write_barrier((void **)&((node *)roots[0])->left, target);

    /* Enough garbage to fill a nursery, so a minor collection runs. */
    for (int index = 0; index < 20000; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    lark_collect();

    const node *found = roots[0];
    REQUIRE(found != NULL);
    CHECK(found->value == 1);
    REQUIRE(found->left != NULL);
    CHECK(found->left->value == 22);
    end();
}

/* A chain of old objects each pointing at a young one. Every link must hold.
 * covers: R-2 */
void test_many_old_to_young_pointers_hold(void);
void test_many_old_to_young_pointers_hold(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims, "the collector frees nothing");
    enum { WIDTH = 32 };
    begin(2);

    /* An array of holders, made old by a collection. */
    node *holders = lark_alloc_array(&NODE_TYPE, WIDTH);
    REQUIRE(holders != NULL);
    roots[0] = holders;
    lark_collect();

    /* One young target per element, stored through the barrier. */
    for (int index = 0; index < WIDTH; index += 1) {
        node *target = lark_alloc(&NODE_TYPE);
        REQUIRE(target != NULL);
        target->value = 100 + index;
        roots[1] = target;
        node *items = roots[0];
        lark_write_barrier((void **)&items[index].left, roots[1]);
    }
    roots[1] = NULL;

    /* Fill the nursery, so a minor collection runs over it. */
    for (int index = 0; index < 20000; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    lark_collect();

    const node *items = roots[0];
    REQUIRE(items != NULL);
    for (int index = 0; index < WIDTH; index += 1) {
        REQUIRE(items[index].left != NULL);
        CHECK(items[index].left->value == 100 + index);
    }
    end();
}

/* A field that the barrier cleared drops its target.
 *
 * The barrier records a store. It must not keep an object that a later store
 * removed, or a marked card would hold the heap open for ever.
 * covers: R-2 */
void test_a_cleared_field_through_the_barrier_drops_its_target(void);
void test_a_cleared_field_through_the_barrier_drops_its_target(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims, "the collector frees nothing");
    begin(1);

    node *holder = lark_alloc(&NODE_TYPE);
    REQUIRE(holder != NULL);
    roots[0] = holder;
    lark_collect();

    node *target = lark_alloc(&NODE_TYPE);
    REQUIRE(target != NULL);
    lark_write_barrier((void **)&((node *)roots[0])->left, target);
    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 2u);

    lark_write_barrier((void **)&((node *)roots[0])->left, NULL);
    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 1u);
    end();
}

/* The capability says whether a collector needs the call.
 *
 * Rule R-2. A collector that needs no barrier performs the store and returns,
 * so a program is correct either way. The transpiler reads the flag to decide
 * whether to emit the call at all.
 * covers: R-1, R-2 */
void test_the_capability_says_whether_a_barrier_is_needed(void);
void test_the_capability_says_whether_a_barrier_is_needed(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    lark_gc_caps caps = lark_gc_capabilities();
    /* Only a collector that walks part of the heap needs one. */
    if (caps.write_barrier) {
        CHECK(caps.moving);
    }
    lark_shutdown();
}
