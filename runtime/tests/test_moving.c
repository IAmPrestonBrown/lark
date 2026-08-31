/* What a moving collector must do, and what a non moving one must not.
 *
 * A collector that moves objects rewrites the address in every root and in
 * every managed field. A test proves that by remembering an address before a
 * collection and comparing it after. A collector that does not move proves the
 * opposite: the address is the one the program already holds.
 *
 * Both halves matter. A non moving collector that quietly moved an object
 * would break every raw pointer that generated code holds outside a frame.
 *
 * Test type T8 in docs/test-strategy.md.
 * covers: M-8, M-10, M-27, R-1 */

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

/* A non moving collector leaves every address where it was. Generated code
 * relies on that whenever it holds a pointer that is not in a frame. */
void test_a_non_moving_collector_keeps_every_address(void);
void test_a_non_moving_collector_keeps_every_address(void) {
    SKIP_UNLESS(!lark_gc_capabilities().moving, "this collector moves objects");
    begin(1);

    node *item = lark_alloc(&NODE_TYPE);
    REQUIRE(item != NULL);
    item->value = 3;
    roots[0] = item;
    const void *before = item;

    for (int index = 0; index < 100; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    lark_collect();

    CHECK(roots[0] == before);
    CHECK(item->value == 3);
    end();
}

/* A moving collector writes the new address into the root array. */
void test_a_moving_collector_rewrites_a_global_root(void);
void test_a_moving_collector_rewrites_a_global_root(void) {
    SKIP_UNLESS(lark_gc_capabilities().moving, "this collector does not move");
    begin(1);

    /* Fill the space with garbage first, so the survivor moves rather than
     * landing back where it started. */
    for (int index = 0; index < 100; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    node *item = lark_alloc(&NODE_TYPE);
    REQUIRE(item != NULL);
    item->value = 11;
    roots[0] = item;
    const void *before = item;

    lark_collect();

    CHECK(roots[0] != before);
    REQUIRE(roots[0] != NULL);
    CHECK(((const node *)roots[0])->value == 11);
    end();
}

/* Rule M-10. A shadow stack slot holds the address of the local, so the local
 * itself gets the new address and the program needs no reload. */
void test_a_moving_collector_rewrites_a_local_through_its_slot(void);
void test_a_moving_collector_rewrites_a_local_through_its_slot(void) {
    SKIP_UNLESS(lark_gc_capabilities().moving, "this collector does not move");
    test_start(LARK_ROOTS_SHADOW_STACK);

    for (int index = 0; index < 100; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }

    TEST_FRAME(frame, 1, 0);
    node *local = lark_alloc(&NODE_TYPE);
    frame.s[0] = (void **)&local;
    frame.h.nslots = 1;
    REQUIRE(local != NULL);
    local->value = 21;
    const void *before = local;

    lark_collect();

    /* The collector wrote through the slot, so the local now names the copy. */
    CHECK(local != before);
    CHECK(local->value == 21);

    lark_frame_pop(&frame.h);
    lark_shutdown();
}

/* Rule M-27. A temporary slot holds the value itself, and a moving collector
 * must rewrite it in place. */
void test_a_moving_collector_rewrites_a_temporary(void);
void test_a_moving_collector_rewrites_a_temporary(void) {
    SKIP_UNLESS(lark_gc_capabilities().moving, "this collector does not move");
    test_start(LARK_ROOTS_SHADOW_STACK);

    for (int index = 0; index < 100; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }

    TEST_FRAME(frame, 0, 1);
    frame.t[0] = lark_alloc(&NODE_TYPE);
    REQUIRE(frame.t[0] != NULL);
    ((node *)frame.t[0])->value = 31;
    const void *before = frame.t[0];

    lark_collect();

    CHECK(frame.t[0] != before);
    CHECK(((const node *)frame.t[0])->value == 31);

    lark_frame_pop(&frame.h);
    lark_shutdown();
}

/* A moving collector must follow the field map and rewrite every managed
 * field, or the graph breaks one link away from a root. */
void test_a_moving_collector_rewrites_every_field(void);
void test_a_moving_collector_rewrites_every_field(void) {
    SKIP_UNLESS(lark_gc_capabilities().moving, "this collector does not move");
    begin(1);

    for (int index = 0; index < 100; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }

    node *parent = lark_alloc(&NODE_TYPE);
    node *left = lark_alloc(&NODE_TYPE);
    node *right = lark_alloc(&NODE_TYPE);
    REQUIRE(parent != NULL && left != NULL && right != NULL);
    left->value = 1;
    right->value = 2;
    parent->left = left;
    parent->right = right;
    roots[0] = parent;
    const void *left_before = left;
    const void *right_before = right;

    lark_collect();
    parent = roots[0];

    REQUIRE(parent != NULL);
    REQUIRE(parent->left != NULL && parent->right != NULL);
    CHECK(parent->left != left_before);
    CHECK(parent->right != right_before);
    CHECK(parent->left->value == 1);
    CHECK(parent->right->value == 2);
    end();
}

/* Every element of an array carries the field map, so a moving collector must
 * rewrite the fields of each element, not only the first. */
void test_a_moving_collector_rewrites_every_array_element(void);
void test_a_moving_collector_rewrites_every_array_element(void) {
    enum { COUNT = 16 };
    begin(1);

    node *items = lark_alloc_array(&NODE_TYPE, COUNT);
    REQUIRE(items != NULL);
    roots[0] = items;
    for (int index = 0; index < COUNT; index += 1) {
        node *target = lark_alloc(&NODE_TYPE);
        REQUIRE(target != NULL);
        target->value = 200 + index;
        /* Read the array back each time, because the allocation above can
         * collect and move it. */
        items = roots[0];
        items[index].left = target;
    }

    lark_collect();
    items = roots[0];

    for (int index = 0; index < COUNT; index += 1) {
        REQUIRE(items[index].left != NULL);
        CHECK(items[index].left->value == 200 + index);
    }
    end();
}

/* Rule M-8. A collector that says it cannot follow an interior pointer must
 * answer NULL rather than a wrong address. */
void test_a_moving_collector_refuses_an_interior_pointer(void);
void test_a_moving_collector_refuses_an_interior_pointer(void) {
    SKIP_UNLESS(lark_gc_capabilities().moving, "this collector does not move");
    begin(1);

    char *buffer = lark_alloc_array(&PLAIN_TYPE, 8);
    REQUIRE(buffer != NULL);
    CHECK(lark_base_of(buffer) == NULL);
    CHECK(lark_base_of(buffer + 4) == NULL);
    CHECK(!lark_gc_capabilities().interior_pointers);
    end();
}

/* The values inside an object survive a move byte for byte. */
void test_a_move_copies_every_byte(void);
void test_a_move_copies_every_byte(void) {
    enum { COUNT = 40 };
    begin(1);

    for (int index = 0; index < 100; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }

    plain *items = lark_alloc_array(&PLAIN_TYPE, COUNT);
    REQUIRE(items != NULL);
    for (int index = 0; index < COUNT; index += 1) {
        items[index].value = index * 3;
        items[index].other = index * 7;
    }
    roots[0] = items;

    lark_collect();
    items = roots[0];

    for (int index = 0; index < COUNT; index += 1) {
        CHECK(items[index].value == index * 3);
        CHECK(items[index].other == index * 7);
    }
    end();
}

/* Many collections in a row must leave the graph consistent every time, and
 * a moving collector must not run out of space when the live set is small. */
void test_repeated_moves_stay_consistent(void);
void test_repeated_moves_stay_consistent(void) {
    enum { ROUNDS = 60, LENGTH = 20 };
    begin(1);

    node *head = lark_alloc(&NODE_TYPE);
    REQUIRE(head != NULL);
    roots[0] = head;
    node *tail = head;
    for (int index = 1; index < LENGTH; index += 1) {
        node *next = lark_alloc(&NODE_TYPE);
        REQUIRE(next != NULL);
        next->value = index;
        tail->left = next;
        tail = next;
    }

    for (int round = 0; round < ROUNDS; round += 1) {
        for (int index = 0; index < 30; index += 1) {
            (void)lark_alloc(&NODE_TYPE);
        }
        lark_collect();

        int seen = 0;
        for (const node *walk = roots[0]; walk != NULL; walk = walk->left) {
            CHECK(walk->value == seen);
            seen += 1;
        }
        REQUIRE(seen == LENGTH);
    }
    end();
}
