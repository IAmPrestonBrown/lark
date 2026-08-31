/* The contract that every collector satisfies.
 *
 * Chapter 10 section 3 defines one seam and says that a program links exactly
 * one collector. These tests hold for every collector behind that seam. A new
 * collector that passes them behaves like the ones that came before it, and a
 * collector that fails one has a fault the design does not allow.
 *
 * Test type T8 in docs/test-strategy.md.
 * covers: R-1, R-2, R-4, R-5, M-4, M-5, F-4 */

/* `setenv` and `unsetenv` come from POSIX, not from C11. With `-std=c11` a
 * strict library hides them, so the macro asks for them. It must come before
 * every include. */
#define _POSIX_C_SOURCE 200809L

#include <stdlib.h>
#include <string.h>

#include "lark_test.h"
#include "test_types.h"

/* Reports whether an address sits on a boundary. */
static bool aligned_to(const void *value, size_t to) {
    return (uintptr_t)value % (uintptr_t)to == 0u;
}

void test_the_collector_names_itself(void);
void test_the_collector_names_itself(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    const char *name = lark_gc_name();
    REQUIRE(name != NULL);
    CHECK(name[0] != '\0');
    /* The name is one of the collectors that chapter 10 section 4 lists. */
    CHECK(strcmp(name, "precise-marksweep") == 0 || strcmp(name, "arena") == 0
          || strcmp(name, "semispace") == 0 || strcmp(name, "generational") == 0);
    lark_shutdown();
}

/* Rule R-1. The transpiler reads these at build time, so they must describe
 * the collector rather than contradict it. */
void test_the_capabilities_agree_with_each_other(void);
void test_the_capabilities_agree_with_each_other(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    lark_gc_caps caps = lark_gc_capabilities();

    /* Rule M-5 makes the field map mandatory, so heap tracing is always
     * precise. No collector may say otherwise. */
    CHECK(caps.precise_heap);

    /* A collector that moves must write a new address into every root, and
     * only a precise stack gives it the address of each root. */
    if (caps.moving) {
        CHECK(caps.precise_stack);
        /* Rule M-8 cannot hold for a moving collector, because an interior
         * pointer has no slot that a copy can update. */
        CHECK(!caps.interior_pointers);
    }

    /* A collector that claims interior pointers must answer for one. */
    plain *item = lark_alloc(&PLAIN_TYPE);
    REQUIRE(item != NULL);
    if (caps.interior_pointers) {
        CHECK(lark_base_of(item) == item);
    } else {
        CHECK(lark_base_of(item) == NULL);
    }
    lark_shutdown();
}

void test_an_allocation_is_zeroed_and_aligned(void);
void test_an_allocation_is_zeroed_and_aligned(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    for (int round = 0; round < 64; round += 1) {
        node *item = lark_alloc(&NODE_TYPE);
        REQUIRE(item != NULL);
        /* Rule O-5. A field with no designator is zero. */
        CHECK(item->left == NULL);
        CHECK(item->right == NULL);
        CHECK(item->value == 0);
        CHECK(aligned_to(item, _Alignof(node)));
    }
    lark_shutdown();
}

/* Rule M-4. The header sits before the payload, and it records what the
 * collector needs to walk the object. */
void test_the_header_records_the_type_and_the_count(void);
void test_the_header_records_the_type_and_the_count(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);

    node *single = lark_alloc(&NODE_TYPE);
    REQUIRE(single != NULL);
    CHECK(LARK_HEADER(single)->type == &NODE_TYPE);
    CHECK(LARK_HEADER(single)->count == 1u);

    node *many = lark_alloc_array(&NODE_TYPE, 7);
    REQUIRE(many != NULL);
    CHECK(LARK_HEADER(many)->type == &NODE_TYPE);
    CHECK(LARK_HEADER(many)->count == 7u);
    lark_shutdown();
}

void test_two_allocations_never_share_a_byte(void);
void test_two_allocations_never_share_a_byte(void) {
    enum { COUNT = 200 };
    test_start(LARK_ROOTS_SHADOW_STACK);

    static void *roots[COUNT];
    memset(roots, 0, sizeof roots);
    lark_root_register(roots, COUNT);

    for (int index = 0; index < COUNT; index += 1) {
        node *item = lark_alloc(&NODE_TYPE);
        REQUIRE(item != NULL);
        item->value = index;
        roots[index] = item;
    }
    /* Every object holds the value it was given, so no two overlapped. */
    for (int index = 0; index < COUNT; index += 1) {
        const node *item = roots[index];
        REQUIRE(item != NULL);
        CHECK(item->value == index);
    }
    lark_root_remove(roots);
    lark_shutdown();
}

/* An array of zero elements still yields one object, so a caller never has to
 * test the count before it allocates. */
void test_a_zero_length_array_yields_one_element(void);
void test_a_zero_length_array_yields_one_element(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    node *item = lark_alloc_array(&NODE_TYPE, 0);
    REQUIRE(item != NULL);
    CHECK(LARK_HEADER(item)->count == 1u);
    lark_shutdown();
}

void test_statistics_rise_with_every_allocation(void);
void test_statistics_rise_with_every_allocation(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    lark_gc_stats before = lark_gc_statistics();
    CHECK(before.total_allocations == 0u);
    CHECK(before.collections == 0u);

    for (int index = 0; index < 32; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    lark_gc_stats after = lark_gc_statistics();
    CHECK(after.total_allocations == 32u);
    /* The total never falls, whatever the collector does with the objects. */
    CHECK(after.total_allocations >= before.total_allocations);

    lark_collect();
    CHECK(lark_gc_statistics().collections == 1u);
    /* A collection frees nothing that it allocated afterwards, so the running
     * total keeps every allocation the program ever made. */
    CHECK(lark_gc_statistics().total_allocations == 32u);
    lark_shutdown();
}

/* A second startup after a shutdown starts from an empty heap, so one process
 * runs many tests. */
void test_shutdown_then_startup_gives_an_empty_heap(void);
void test_shutdown_then_startup_gives_an_empty_heap(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    for (int index = 0; index < 16; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    CHECK(lark_gc_statistics().total_allocations == 16u);
    lark_shutdown();

    test_start(LARK_ROOTS_SHADOW_STACK);
    lark_gc_stats stats = lark_gc_statistics();
    CHECK(stats.total_allocations == 0u);
    CHECK(stats.live_objects == 0u);
    CHECK(stats.collections == 0u);
    lark_shutdown();
}

/* A collection with no root and no object is legal and changes nothing. */
void test_a_collection_of_an_empty_heap_is_safe(void);
void test_a_collection_of_an_empty_heap_is_safe(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    lark_collect();
    lark_collect();
    lark_gc_stats stats = lark_gc_statistics();
    CHECK(stats.collections == 2u);
    CHECK(stats.live_objects == 0u);
    lark_shutdown();
}

/* An object that only a temporary slot holds survives, because rule M-27 puts
 * every fresh allocation in one before anything else can allocate. */
void test_every_root_kind_keeps_an_object(void);
void test_every_root_kind_keeps_an_object(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims, "the collector frees nothing");
    test_start(LARK_ROOTS_SHADOW_STACK);

    static void *roots[1];
    roots[0] = NULL;
    lark_root_register(roots, 1);

    TEST_FRAME(frame, 1, 1);
    node *local = lark_alloc(&NODE_TYPE);
    frame.s[0] = (void **)&local;
    frame.h.nslots = 1;
    REQUIRE(local != NULL);

    node *global = lark_alloc(&NODE_TYPE);
    REQUIRE(global != NULL);
    roots[0] = global;

    frame.t[0] = lark_alloc(&NODE_TYPE);
    REQUIRE(frame.t[0] != NULL);

    for (int index = 0; index < 40; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    lark_collect();

    /* The global array, the shadow stack slot, and the temporary. */
    CHECK(lark_gc_statistics().live_objects == 3u);
    CHECK(local != NULL);
    CHECK(roots[0] != NULL);
    CHECK(frame.t[0] != NULL);

    lark_frame_pop(&frame.h);
    lark_root_remove(roots);
    lark_shutdown();
}

/* Rule R-5. A moving collector accepts rule M-10 shadow stack roots alone.
 * The runtime stops such a build at startup, so a test proves the rule by
 * asking the predicate rather than by starting one. */
void test_a_moving_collector_refuses_a_conservative_scan(void);
void test_a_moving_collector_refuses_a_conservative_scan(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    bool moving = lark_gc_capabilities().moving;
    CHECK(test_roots_supported(LARK_ROOTS_SHADOW_STACK));
    CHECK(test_roots_supported(LARK_ROOTS_CONSERVATIVE) == !moving);
    lark_shutdown();
}

/* Rule F-4. The variable turns torture mode on for a binary that the build did
 * not configure for it, so a program ships once and runs either way. */
void test_the_environment_variable_turns_on_torture_mode(void);
void test_the_environment_variable_turns_on_torture_mode(void) {
    /* The build leaves torture off. */
    lark_gc_config off = lark_gc_config_default();
    off.roots = LARK_ROOTS_SHADOW_STACK;
    CHECK(!off.torture);

    setenv("LARK_GC_TORTURE", "1", 1);
    lark_startup_with(off);
    /* Every allocation now collects, so a handful of them raise the count. */
    for (int index = 0; index < 4; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    CHECK(lark_gc_statistics().collections >= 4u);
    lark_shutdown();

    /* The value `0` leaves the build setting alone. */
    setenv("LARK_GC_TORTURE", "0", 1);
    lark_startup_with(off);
    for (int index = 0; index < 4; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    CHECK(lark_gc_statistics().collections == 0u);
    lark_shutdown();

    unsetenv("LARK_GC_TORTURE");
}
