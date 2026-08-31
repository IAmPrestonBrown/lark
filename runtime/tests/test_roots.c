/* The root sets that rule M-9 lists. */

#include <string.h>

#include "lark_test.h"
#include "test_types.h"

static void *globals[2];

/* covers: M-9 */
void test_a_global_root_keeps_an_object_alive(void);
void test_a_global_root_keeps_an_object_alive(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims,
                "the collector frees nothing");
    test_start(LARK_ROOTS_SHADOW_STACK);
    memset(globals, 0, sizeof globals);
    lark_root_register(globals, 2);

    node *item = lark_alloc(&NODE_TYPE);
    REQUIRE(item != NULL);
    globals[0] = item;

    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 1u);

    globals[0] = NULL;
    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 0u);

    lark_root_remove(globals);
    lark_shutdown();
}

void test_a_removed_root_array_stops_keeping_an_object(void);
void test_a_removed_root_array_stops_keeping_an_object(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims,
                "the collector frees nothing");
    test_start(LARK_ROOTS_SHADOW_STACK);
    memset(globals, 0, sizeof globals);
    lark_root_register(globals, 2);

    globals[0] = lark_alloc(&NODE_TYPE);
    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 1u);

    lark_root_remove(globals);
    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 0u);

    lark_shutdown();
}

/* covers: M-10, M-11 */
void test_a_shadow_stack_slot_keeps_an_object_alive(void);
void test_a_shadow_stack_slot_keeps_an_object_alive(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims,
                "the collector frees nothing");
    test_start(LARK_ROOTS_SHADOW_STACK);

    /* This is the frame that rule M-10 emits for a function with one managed
     * local. The local keeps its own name, and the slot holds its address. */
    TEST_FRAME(frame, 1, 1);

    node *kept = lark_alloc(&NODE_TYPE);
    frame.s[0] = (void **)&kept;
    frame.h.nslots = 1;
    REQUIRE(kept != NULL);
    kept->value = 7;

    for (int index = 0; index < 30; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    lark_collect();

    CHECK(lark_gc_statistics().live_objects == 1u);
    CHECK(kept->value == 7);

    lark_frame_pop(&frame.h);
    lark_shutdown();
}

/* covers: M-12 */
void test_a_popped_frame_stops_keeping_an_object(void);
void test_a_popped_frame_stops_keeping_an_object(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims,
                "the collector frees nothing");
    test_start(LARK_ROOTS_SHADOW_STACK);

    node *dropped = NULL;
    TEST_FRAME(frame, 1, 0);
    dropped = lark_alloc(&NODE_TYPE);
    frame.s[0] = (void **)&dropped;
    frame.h.nslots = 1;
    lark_frame_pop(&frame.h);

    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 0u);
    CHECK(dropped != NULL);

    lark_shutdown();
}

/* covers: M-13 */
void test_a_conservative_scan_finds_a_stack_pointer(void);
void test_a_conservative_scan_finds_a_stack_pointer(void) {
    SKIP_UNLESS(test_roots_supported(LARK_ROOTS_CONSERVATIVE),
                "a moving collector needs precise roots");
    test_start(LARK_ROOTS_CONSERVATIVE);

    /* No frame, no global root. The only reference is a local variable, and
     * rule M-13 finds it by scanning the machine stack. */
    node *item = lark_alloc(&NODE_TYPE);
    REQUIRE(item != NULL);
    item->value = 99;

    for (int index = 0; index < 30; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    lark_collect();

    CHECK(lark_gc_statistics().live_objects >= 1u);
    CHECK(item->value == 99);
    CHECK(lark_base_of(item) == item);

    lark_shutdown();
}

/* covers: M-27 */
void test_a_temporary_slot_keeps_a_fresh_object_alive(void);
void test_a_temporary_slot_keeps_a_fresh_object_alive(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims,
                "the collector frees nothing");
    test_start(LARK_ROOTS_SHADOW_STACK);
    TEST_FRAME(frame, 0, 1);

    /* This is what `f(new A(), new B())` emits for the first argument. The
     * object belongs to no local, and the temporary slot keeps it. */
    frame.t[0] = lark_alloc(&NODE_TYPE);
    REQUIRE(frame.t[0] != NULL);
    ((node *)frame.t[0])->value = 3;

    for (int index = 0; index < 20; index += 1) {
        (void)lark_alloc(&NODE_TYPE);
    }
    lark_collect();

    CHECK(lark_gc_statistics().live_objects == 1u);
    CHECK(((node *)frame.t[0])->value == 3);

    lark_frame_pop(&frame.h);
    lark_shutdown();
}

/* covers: M-14 */
void test_both_root_modes_accept_the_same_program(void);
void test_both_root_modes_accept_the_same_program(void) {
    for (int pass = 0; pass < 2; pass += 1) {
        lark_roots mode = pass == 0 ? LARK_ROOTS_SHADOW_STACK : LARK_ROOTS_CONSERVATIVE;
        if (!test_roots_supported(mode)) {
            continue;
        }
        test_start(mode);
        memset(globals, 0, sizeof globals);
        lark_root_register(globals, 2);

        globals[0] = lark_alloc(&NODE_TYPE);
        REQUIRE(globals[0] != NULL);
        ((node *)globals[0])->value = 5;
        lark_collect();
        CHECK(((node *)globals[0])->value == 5);

        lark_root_remove(globals);
        lark_shutdown();
    }
}

/* covers: M-15 */
void test_a_long_jump_restores_the_shadow_stack(void);
void test_a_long_jump_restores_the_shadow_stack(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);

    TEST_FRAME(outer, 1, 0);

    lark_jmp_buf buffer;
    if (lark_setjmp(&buffer) == 0) {
        TEST_FRAME(inner, 1, 0);
        /* The jump skips the pop for the inner frame. */
        lark_longjmp(&buffer, 1);
    }

    /* Rule M-15. The head is the outer frame again, not the dead inner one. */
    CHECK(lark_frame_top() == &outer.h);
    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 0u);

    lark_frame_pop(&outer.h);
    lark_shutdown();
}
