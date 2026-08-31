/* Reachability over the graph shapes that a real program builds.
 *
 * The mark phase and the copy phase both walk an object graph. A shape that
 * the walk handles badly shows up as a lost object, a duplicated object, or a
 * walk that never ends. Each test builds one shape, collects, and proves that
 * the graph came through whole.
 *
 * Test type T8 in docs/test-strategy.md.
 * covers: M-5, M-9 */

#include <string.h>

#include "lark_test.h"
#include "test_types.h"

static void *roots[8];

static void begin(size_t count) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    memset(roots, 0, sizeof roots);
    lark_root_register(roots, count);
}

static void end(void) {
    lark_root_remove(roots);
    lark_shutdown();
}

/* A chain deeper than any recursive marker can hold on the machine stack.
 *
 * The collector must walk it with a worklist of its own. A depth of ten
 * thousand overflows a stack that recurses once per link. */
void test_a_deep_chain_survives(void);
void test_a_deep_chain_survives(void) {
    enum { DEPTH = 10000 };
    begin(2);

    node *head = lark_alloc(&NODE_TYPE);
    REQUIRE(head != NULL);
    head->value = 0;
    roots[0] = head;
    /* The tail is a root of its own. A chain this long fills a nursery, so a
     * collection runs while the loop builds it, and a raw local would then
     * name an object that moved. Generated code has no such local, because
     * rule M-10 makes the shadow stack slot the address of the local. */
    roots[1] = head;

    for (int index = 1; index < DEPTH; index += 1) {
        node *next = lark_alloc(&NODE_TYPE);
        REQUIRE(next != NULL);
        next->value = index;
        /* Read the tail back, because the allocation above can move it. */
        ((node *)roots[1])->left = next;
        roots[1] = next;
    }

    lark_collect();
    head = roots[0];

    CHECK(lark_gc_statistics().live_objects == (size_t)DEPTH);
    int seen = 0;
    for (const node *walk = head; walk != NULL; walk = walk->left) {
        CHECK(walk->value == seen);
        seen += 1;
    }
    CHECK(seen == DEPTH);
    end();
}

/* One object that many others point at. A collector must place it once and
 * give every path the same address. */
void test_a_shared_object_stays_one_object(void);
void test_a_shared_object_stays_one_object(void) {
    enum { WIDTH = 64 };
    begin(2);

    node *shared = lark_alloc(&NODE_TYPE);
    REQUIRE(shared != NULL);
    shared->value = 999;

    node *holders = lark_alloc_array(&NODE_TYPE, WIDTH);
    REQUIRE(holders != NULL);
    for (int index = 0; index < WIDTH; index += 1) {
        holders[index].left = shared;
        holders[index].value = index;
    }
    roots[0] = holders;
    roots[1] = shared;

    lark_collect();
    holders = roots[0];
    shared = roots[1];

    /* The array and the shared object, so two objects. */
    CHECK(lark_gc_statistics().live_objects == 2u);
    for (int index = 0; index < WIDTH; index += 1) {
        CHECK(holders[index].left == shared);
        CHECK(holders[index].value == index);
    }
    CHECK(shared->value == 999);
    end();
}

/* A diamond. Two paths reach the same object, and both must still agree
 * afterwards. A moving collector that copies twice fails this. */
void test_a_diamond_keeps_one_identity(void);
void test_a_diamond_keeps_one_identity(void) {
    begin(1);

    node *top = lark_alloc(&NODE_TYPE);
    node *left = lark_alloc(&NODE_TYPE);
    node *right = lark_alloc(&NODE_TYPE);
    node *bottom = lark_alloc(&NODE_TYPE);
    REQUIRE(top != NULL && left != NULL && right != NULL && bottom != NULL);

    bottom->value = 4;
    top->left = left;
    top->right = right;
    left->left = bottom;
    right->left = bottom;
    roots[0] = top;

    lark_collect();
    top = roots[0];

    CHECK(lark_gc_statistics().live_objects == 4u);
    REQUIRE(top->left != NULL && top->right != NULL);
    CHECK(top->left->left == top->right->left);
    REQUIRE(top->left->left != NULL);
    CHECK(top->left->left->value == 4);
    end();
}

/* An object that points at itself. The walk must stop. */
void test_a_self_reference_terminates(void);
void test_a_self_reference_terminates(void) {
    begin(1);

    node *item = lark_alloc(&NODE_TYPE);
    REQUIRE(item != NULL);
    item->left = item;
    item->right = item;
    item->value = 7;
    roots[0] = item;

    lark_collect();
    item = roots[0];

    CHECK(lark_gc_statistics().live_objects == 1u);
    CHECK(item->left == item);
    CHECK(item->right == item);
    CHECK(item->value == 7);
    end();
}

/* A ring of many objects, each pointing at the next and the last at the
 * first. Nothing in it is a leaf. */
void test_a_long_cycle_survives_whole(void);
void test_a_long_cycle_survives_whole(void) {
    enum { LENGTH = 500 };
    begin(1);

    node *first = lark_alloc(&NODE_TYPE);
    REQUIRE(first != NULL);
    first->value = 0;
    roots[0] = first;

    node *previous = first;
    for (int index = 1; index < LENGTH; index += 1) {
        node *next = lark_alloc(&NODE_TYPE);
        REQUIRE(next != NULL);
        next->value = index;
        previous->left = next;
        previous = next;
    }
    previous->left = roots[0];

    lark_collect();
    first = roots[0];

    CHECK(lark_gc_statistics().live_objects == (size_t)LENGTH);
    const node *walk = first;
    for (int index = 0; index < LENGTH; index += 1) {
        CHECK(walk->value == index);
        walk = walk->left;
    }
    /* The ring closed, so the walk came back to where it started. */
    CHECK(walk == first);
    end();
}

/* A binary tree. Every node holds two children, so the walk branches. */
void test_a_binary_tree_survives(void);
void test_a_binary_tree_survives(void) {
    enum { DEPTH = 12 };  /* 4095 nodes */
    begin(1);

    /* Build the tree from a stack of pending nodes, so the test itself does
     * not recurse. Each entry is a node that still needs its children. */
    static node *pending[8192];
    size_t head = 0;
    size_t tail = 0;
    static int depth_of[8192];

    node *root = lark_alloc(&NODE_TYPE);
    REQUIRE(root != NULL);
    roots[0] = root;
    pending[tail] = root;
    depth_of[tail] = 1;
    tail += 1;

    size_t built = 1;
    while (head < tail) {
        node *item = pending[head];
        int depth = depth_of[head];
        head += 1;
        if (depth >= DEPTH) {
            continue;
        }
        node *left = lark_alloc(&NODE_TYPE);
        node *right = lark_alloc(&NODE_TYPE);
        REQUIRE(left != NULL && right != NULL);
        left->value = depth + 1;
        right->value = depth + 1;
        item->left = left;
        item->right = right;
        built += 2;
        pending[tail] = left;
        depth_of[tail] = depth + 1;
        tail += 1;
        pending[tail] = right;
        depth_of[tail] = depth + 1;
        tail += 1;
        /* The stack holds raw pointers, and a collection would move them.
         * Nothing collects here, because the test asks for it only once. */
    }

    lark_collect();
    root = roots[0];

    CHECK(lark_gc_statistics().live_objects == built);
    /* Walk the leftmost spine, which is as deep as the tree. */
    int depth = 1;
    for (const node *walk = root; walk->left != NULL; walk = walk->left) {
        depth += 1;
        CHECK(walk->left->value == depth);
    }
    CHECK(depth == DEPTH);
    end();
}

/* A large object, larger than every size class, with a managed field. */
void test_a_large_object_keeps_its_field(void);
void test_a_large_object_keeps_its_field(void) {
    begin(1);

    big *item = lark_alloc(&BIG_TYPE);
    REQUIRE(item != NULL);
    node *target = lark_alloc(&NODE_TYPE);
    REQUIRE(target != NULL);
    target->value = 55;
    item->next = (big *)(void *)target;
    memset(item->bytes, 'x', sizeof item->bytes);
    roots[0] = item;

    lark_collect();
    item = roots[0];

    CHECK(lark_gc_statistics().live_objects == 2u);
    REQUIRE(item->next != NULL);
    CHECK(((const node *)(const void *)item->next)->value == 55);
    CHECK(item->bytes[0] == 'x');
    CHECK(item->bytes[sizeof item->bytes - 1] == 'x');
    end();
}

/* A graph that survives many collections must come through each one whole. */
void test_a_graph_survives_repeated_collections(void);
void test_a_graph_survives_repeated_collections(void) {
    enum { LENGTH = 40, ROUNDS = 25 };
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
        /* Allocate garbage between the collections, so each one has work. */
        for (int index = 0; index < 50; index += 1) {
            (void)lark_alloc(&NODE_TYPE);
        }
        lark_collect();

        head = roots[0];
        int seen = 0;
        for (const node *walk = head; walk != NULL; walk = walk->left) {
            CHECK(walk->value == seen);
            seen += 1;
        }
        REQUIRE(seen == LENGTH);
    }
    CHECK(lark_gc_statistics().collections == (size_t)ROUNDS);
    end();
}

/* A field that a program clears before a collection must not keep its old
 * target alive. */
void test_a_cleared_field_drops_its_target(void);
void test_a_cleared_field_drops_its_target(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims, "the collector frees nothing");
    begin(1);

    node *holder = lark_alloc(&NODE_TYPE);
    node *target = lark_alloc(&NODE_TYPE);
    REQUIRE(holder != NULL && target != NULL);
    holder->left = target;
    roots[0] = holder;

    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 2u);

    holder = roots[0];
    holder->left = NULL;
    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 1u);
    end();
}
