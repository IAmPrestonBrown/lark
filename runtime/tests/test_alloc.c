/* Allocation, headers, size classes, and statistics. */

#include <string.h>

#include "lark_test.h"
#include "test_types.h"

/* covers: O-4 */
void test_alloc_returns_zeroed_payload(void);
void test_alloc_returns_zeroed_payload(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    node *item = lark_alloc(&NODE_TYPE);
    REQUIRE(item != NULL);
    CHECK(item->left == NULL);
    CHECK(item->right == NULL);
    CHECK(item->value == 0);
    lark_shutdown();
}

/* covers: M-4, M-5 */
void test_alloc_records_the_type(void);
void test_alloc_records_the_type(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    node *item = lark_alloc(&NODE_TYPE);
    REQUIRE(item != NULL);
    const lark_header *header = LARK_HEADER(item);
    CHECK(header->type == &NODE_TYPE);
    CHECK(header->count == 1u);
    lark_shutdown();
}

/* covers: O-6 */
void test_alloc_array_reserves_every_element(void);
void test_alloc_array_reserves_every_element(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    node *items = lark_alloc_array(&NODE_TYPE, 8);
    REQUIRE(items != NULL);
    CHECK(LARK_HEADER(items)->count == 8u);
    for (int index = 0; index < 8; index += 1) {
        CHECK(items[index].value == 0);
        items[index].value = index;
    }
    for (int index = 0; index < 8; index += 1) {
        CHECK(items[index].value == index);
    }
    lark_shutdown();
}

void test_two_allocations_do_not_overlap(void);
void test_two_allocations_do_not_overlap(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    node *first = lark_alloc(&NODE_TYPE);
    node *second = lark_alloc(&NODE_TYPE);
    REQUIRE(first != NULL && second != NULL);
    CHECK(first != second);
    first->value = 11;
    second->value = 22;
    CHECK(first->value == 11);
    CHECK(second->value == 22);
    lark_shutdown();
}

void test_a_large_object_gets_a_page_of_its_own(void);
void test_a_large_object_gets_a_page_of_its_own(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    big *item = lark_alloc(&BIG_TYPE);
    REQUIRE(item != NULL);
    /* Every byte belongs to this object, so a write to the last one is safe. */
    item->bytes[0] = 'a';
    item->bytes[sizeof item->bytes - 1] = 'z';
    CHECK(item->bytes[0] == 'a');
    CHECK(item->bytes[sizeof item->bytes - 1] == 'z');
    lark_shutdown();
}

void test_statistics_count_every_allocation(void);
void test_statistics_count_every_allocation(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    for (int index = 0; index < 100; index += 1) {
        (void)lark_alloc(&PLAIN_TYPE);
    }
    lark_gc_stats stats = lark_gc_statistics();
    CHECK(stats.total_allocations == 100u);
    CHECK(stats.heap_bytes > 0u);
    lark_shutdown();
}

/* covers: O-23 */
void test_an_interface_table_finds_its_method_table(void);
void test_an_interface_table_finds_its_method_table(void) {
    static const char greet_id = 0;
    static const char other_id = 0;
    static const int greet_vtable = 7;
    static const lark_itable_ent entries[] = {
        { &greet_id, &greet_vtable },
    };
    const lark_typeinfo type = {
        "witness", sizeof(int), (uint32_t)_Alignof(int), 0, NULL, 1, entries,
    };

    CHECK(lark_itable_find(&type, &greet_id) == &greet_vtable);
    CHECK(lark_itable_find(&type, &other_id) == NULL);
    CHECK(lark_itable_find(NULL, &greet_id) == NULL);
    CHECK(lark_itable_find(&type, NULL) == NULL);
}

void test_many_allocations_of_every_size(void);
void test_many_allocations_of_every_size(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    static const size_t SIZES[] = { 1, 2, 4, 8, 16, 64, 256, 1024 };
    for (size_t index = 0; index < sizeof SIZES / sizeof SIZES[0]; index += 1) {
        void *item = lark_alloc_array(&PLAIN_TYPE, SIZES[index]);
        CHECK(item != NULL);
        if (item != NULL) {
            memset(item, 0x5a, sizeof(plain) * SIZES[index]);
        }
    }
    lark_shutdown();
}
