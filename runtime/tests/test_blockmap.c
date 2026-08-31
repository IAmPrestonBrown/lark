/* The block map, and the interior pointers that rule M-6 allows. */

#include "lark_test.h"
#include "test_types.h"

void test_base_of_finds_an_object_from_its_own_address(void);
void test_base_of_finds_an_object_from_its_own_address(void) {
    SKIP_UNLESS(lark_gc_capabilities().interior_pointers,
                "rule M-8 does not hold for this collector");
    test_start(LARK_ROOTS_SHADOW_STACK);
    node *item = lark_alloc(&NODE_TYPE);
    REQUIRE(item != NULL);
    CHECK(lark_base_of(item) == item);
    lark_shutdown();
}

/* covers: M-6, M-7 */
void test_base_of_finds_an_object_from_an_interior_address(void);
void test_base_of_finds_an_object_from_an_interior_address(void) {
    SKIP_UNLESS(lark_gc_capabilities().interior_pointers,
                "rule M-8 does not hold for this collector");
    test_start(LARK_ROOTS_SHADOW_STACK);
    char *buffer = lark_alloc_array(&PLAIN_TYPE, 64);
    REQUIRE(buffer != NULL);
    /* `&buf[3]` is an interior pointer, and rule M-6 makes it a first class
     * value. Rule M-7 resolves it in constant time. */
    for (size_t offset = 0; offset < sizeof(plain) * 64u; offset += 7u) {
        CHECK(lark_base_of(buffer + offset) == buffer);
    }
    lark_shutdown();
}

/* covers: M-7 */
void test_base_of_works_inside_a_large_object(void);
void test_base_of_works_inside_a_large_object(void) {
    SKIP_UNLESS(lark_gc_capabilities().interior_pointers,
                "rule M-8 does not hold for this collector");
    test_start(LARK_ROOTS_SHADOW_STACK);
    big *item = lark_alloc(&BIG_TYPE);
    REQUIRE(item != NULL);
    /* The object spans more than one aligned page, so the map must register
     * every page of it. */
    CHECK(lark_base_of(&item->bytes[0]) == item);
    CHECK(lark_base_of(&item->bytes[70000]) == item);
    CHECK(lark_base_of(&item->bytes[sizeof item->bytes - 1]) == item);
    lark_shutdown();
}

void test_base_of_returns_null_outside_the_heap(void);
void test_base_of_returns_null_outside_the_heap(void) {
    SKIP_UNLESS(lark_gc_capabilities().interior_pointers,
                "rule M-8 does not hold for this collector");
    test_start(LARK_ROOTS_SHADOW_STACK);
    int on_the_stack = 0;
    CHECK(lark_base_of(NULL) == NULL);
    CHECK(lark_base_of(&on_the_stack) == NULL);
    CHECK(lark_base_of((void *)(uintptr_t)0x1234u) == NULL);
    lark_shutdown();
}

void test_base_of_returns_null_for_a_freed_object(void);
void test_base_of_returns_null_for_a_freed_object(void) {
    SKIP_UNLESS(lark_gc_capabilities().interior_pointers,
                "rule M-8 does not hold for this collector");
    SKIP_UNLESS(lark_gc_capabilities().reclaims,
                "the collector frees nothing");
    test_start(LARK_ROOTS_SHADOW_STACK);
    /* Two objects, so the page survives the sweep and the map still holds it. */
    node *keep = lark_alloc(&NODE_TYPE);
    node *drop = lark_alloc(&NODE_TYPE);
    REQUIRE(keep != NULL && drop != NULL);

    void *slots[1];
    slots[0] = keep;
    lark_root_register(slots, 1);
    lark_collect();

    CHECK(lark_base_of(keep) == keep);
    CHECK(lark_base_of(drop) == NULL);

    lark_root_remove(slots);
    lark_shutdown();
}
