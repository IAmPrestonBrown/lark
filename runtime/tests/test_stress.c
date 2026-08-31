/* Randomized churn against the collector.
 *
 * A hand written test proves one shape. These tests build many shapes from a
 * seeded generator, and check the same invariant after every round: every
 * object that a root still reaches holds the value it was given, and the live
 * count matches a hand count of the graph.
 *
 * The generator is written out here rather than taken from the platform, so a
 * failure reproduces exactly from its seed on any machine.
 *
 * Test type T8 in docs/test-strategy.md.
 * covers: M-9, M-16, M-26 */

#include <string.h>

#include "lark_test.h"
#include "test_types.h"

/* A linear congruential generator. The constants are the ones that C99 gives
 * as an example, so the sequence is the same everywhere. */
static uint32_t seed_state;

static void seed(uint32_t value) {
    seed_state = value;
}

static uint32_t next_random(void) {
    seed_state = seed_state * 1103515245u + 12345u;
    return (seed_state >> 16) & 0x7fffu;
}

static uint32_t next_below(uint32_t bound) {
    return next_random() % bound;
}

enum { SLOTS = 64 };
static void *roots[SLOTS];

static void begin(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    memset(roots, 0, sizeof roots);
    lark_root_register(roots, SLOTS);
}

static void end(void) {
    lark_root_remove(roots);
    lark_shutdown();
}

/* Counts the objects that the roots reach, without recursion.
 *
 * The walk marks nothing, so it uses the `right` field of a scratch list. It
 * instead compares addresses against a seen list, which is enough for the
 * small graphs that these tests build. */
static size_t reachable_count(void **seen, size_t capacity) {
    size_t found = 0;
    size_t scanned = 0;

    for (size_t index = 0; index < SLOTS; index += 1) {
        void *value = roots[index];
        if (value == NULL) {
            continue;
        }
        bool already = false;
        for (size_t other = 0; other < found; other += 1) {
            if (seen[other] == value) {
                already = true;
                break;
            }
        }
        if (!already && found < capacity) {
            seen[found] = value;
            found += 1;
        }
    }

    while (scanned < found) {
        const node *item = seen[scanned];
        scanned += 1;
        void *links[2] = { item->left, item->right };
        for (int which = 0; which < 2; which += 1) {
            void *value = links[which];
            if (value == NULL) {
                continue;
            }
            bool already = false;
            for (size_t other = 0; other < found; other += 1) {
                if (seen[other] == value) {
                    already = true;
                    break;
                }
            }
            if (!already && found < capacity) {
                seen[found] = value;
                found += 1;
            }
        }
    }
    return found;
}

/* Allocates, drops, and links at random, and collects between the rounds.
 *
 * After each collection, every root that still holds an object must hold the
 * value that the round gave it. A collector that frees a reachable object or
 * that loses a rewrite fails here.
 */
void test_random_churn_keeps_every_reachable_value(void);
void test_random_churn_keeps_every_reachable_value(void) {
    enum { ROUNDS = 300 };
    static int expected[SLOTS];

    begin();
    seed(20260830u);
    memset(expected, 0, sizeof expected);

    for (int round = 0; round < ROUNDS; round += 1) {
        uint32_t slot = next_below(SLOTS);
        uint32_t action = next_below(10u);

        if (action < 5u) {
            /* Put a fresh object in the slot. */
            node *item = lark_alloc(&NODE_TYPE);
            REQUIRE(item != NULL);
            item->value = round + 1;
            roots[slot] = item;
            expected[slot] = round + 1;
        } else if (action < 7u) {
            /* Drop whatever the slot held. */
            roots[slot] = NULL;
            expected[slot] = 0;
        } else if (action < 9u) {
            /* Link one slot to another, which makes a shared object. */
            uint32_t other = next_below(SLOTS);
            if (roots[slot] != NULL && roots[other] != NULL) {
                ((node *)roots[slot])->left = roots[other];
            }
        } else {
            /* Allocate garbage that no root holds. */
            for (int index = 0; index < 5; index += 1) {
                (void)lark_alloc(&NODE_TYPE);
            }
        }

        if (round % 7 == 0) {
            lark_collect();
            for (size_t index = 0; index < SLOTS; index += 1) {
                if (expected[index] != 0) {
                    REQUIRE(roots[index] != NULL);
                    REQUIRE(((const node *)roots[index])->value == expected[index]);
                } else {
                    CHECK(roots[index] == NULL);
                }
            }
        }
    }
    end();
}

/* The live count after a collection must equal a hand count of the graph. */
void test_the_live_count_matches_the_graph(void);
void test_the_live_count_matches_the_graph(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims, "the collector frees nothing");
    enum { ROUNDS = 40, CAPACITY = 512 };
    static void *seen[CAPACITY];

    begin();
    seed(19700101u);

    for (int round = 0; round < ROUNDS; round += 1) {
        for (int index = 0; index < 12; index += 1) {
            uint32_t slot = next_below(SLOTS);
            if (next_below(3u) == 0u) {
                roots[slot] = NULL;
            } else {
                node *item = lark_alloc(&NODE_TYPE);
                REQUIRE(item != NULL);
                roots[slot] = item;
                uint32_t other = next_below(SLOTS);
                if (roots[other] != NULL && roots[other] != item) {
                    item->left = roots[other];
                }
            }
        }
        lark_collect();

        size_t counted = reachable_count(seen, CAPACITY);
        REQUIRE(counted < CAPACITY);
        CHECK(lark_gc_statistics().live_objects == counted);
    }
    end();
}

/* Mixed sizes, so the collector uses every size class and the large path. */
void test_churn_over_mixed_sizes(void);
void test_churn_over_mixed_sizes(void) {
    enum { ROUNDS = 120 };
    begin();
    seed(31415926u);

    for (int round = 0; round < ROUNDS; round += 1) {
        uint32_t slot = next_below(SLOTS);
        uint32_t kind = next_below(10u);

        if (kind < 6u) {
            node *item = lark_alloc(&NODE_TYPE);
            REQUIRE(item != NULL);
            item->value = round;
            roots[slot] = item;
        } else if (kind < 9u) {
            /* An array, whose length varies over the size classes. */
            size_t count = 1u + next_below(48u);
            node *items = lark_alloc_array(&NODE_TYPE, count);
            REQUIRE(items != NULL);
            items[0].value = round;
            roots[slot] = items;
        } else {
            /* One object larger than every size class. */
            big *item = lark_alloc(&BIG_TYPE);
            REQUIRE(item != NULL);
            item->bytes[0] = (char)round;
            roots[slot] = item;
        }

        if (round % 11 == 0) {
            lark_collect();
        }
    }
    lark_collect();
    /* Nothing crashed, and every slot still points at an object or at NULL. */
    for (size_t index = 0; index < SLOTS; index += 1) {
        if (roots[index] != NULL && lark_gc_capabilities().interior_pointers) {
            CHECK(lark_base_of(roots[index]) == roots[index]);
        }
    }
    end();
}

/* The heap must not grow without bound when the live set stays small. A
 * collector that reclaims keeps the heap near the live set. */
void test_a_small_live_set_keeps_the_heap_small(void);
void test_a_small_live_set_keeps_the_heap_small(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims, "the collector frees nothing");
    enum { ROUNDS = 200 };
    begin();

    node *kept = lark_alloc(&NODE_TYPE);
    REQUIRE(kept != NULL);
    kept->value = 1;
    roots[0] = kept;

    for (int round = 0; round < ROUNDS; round += 1) {
        for (int index = 0; index < 50; index += 1) {
            (void)lark_alloc(&NODE_TYPE);
        }
        lark_collect();
        CHECK(lark_gc_statistics().live_objects == 1u);
    }

    CHECK(lark_gc_statistics().total_allocations == (size_t)(ROUNDS * 50 + 1));
    REQUIRE(roots[0] != NULL);
    CHECK(((const node *)roots[0])->value == 1);
    end();
}

/* Torture mode collects at every safepoint. A program that survives it holds
 * every root it needs, at every point where a collection can start. */
void test_torture_mode_survives_a_long_build(void);
void test_torture_mode_survives_a_long_build(void) {
    enum { LENGTH = 60 };
    lark_gc_config config = lark_gc_config_default();
    config.roots = LARK_ROOTS_SHADOW_STACK;
    config.torture = true;
    lark_startup_with(config);

    memset(roots, 0, sizeof roots);
    lark_root_register(roots, SLOTS);

    /* Build a chain with a collection between every link. The head stays in a
     * root, and each new link goes into a root before it is attached. */
    node *head = lark_alloc(&NODE_TYPE);
    REQUIRE(head != NULL);
    roots[0] = head;

    for (int index = 1; index < LENGTH; index += 1) {
        node *next = lark_alloc(&NODE_TYPE);
        REQUIRE(next != NULL);
        roots[1] = next;
        next->value = index;
        /* Read the tail back through the root each time, because every
         * allocation above ran a collection. */
        node *tail = roots[0];
        for (int step = 1; step < index; step += 1) {
            tail = tail->left;
        }
        tail->left = roots[1];
        roots[1] = NULL;
    }

    int seen = 0;
    for (const node *walk = roots[0]; walk != NULL; walk = walk->left) {
        CHECK(walk->value == seen);
        seen += 1;
    }
    CHECK(seen == LENGTH);
    CHECK(lark_gc_statistics().collections >= (size_t)LENGTH);

    lark_root_remove(roots);
    lark_shutdown();
}
