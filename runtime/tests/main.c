/* Runs every runtime test. Test type T8 in docs/test-strategy.md. */

#include <stdio.h>

#include "lark_rt.h"
#include "lark_test.h"

void test_alloc_returns_zeroed_payload(void);
void test_alloc_records_the_type(void);
void test_alloc_array_reserves_every_element(void);
void test_two_allocations_do_not_overlap(void);
void test_a_large_object_gets_a_page_of_its_own(void);
void test_statistics_count_every_allocation(void);
void test_many_allocations_of_every_size(void);
void test_an_interface_table_finds_its_method_table(void);

void test_base_of_finds_an_object_from_its_own_address(void);
void test_base_of_finds_an_object_from_an_interior_address(void);
void test_base_of_works_inside_a_large_object(void);
void test_base_of_returns_null_outside_the_heap(void);
void test_base_of_returns_null_for_a_freed_object(void);

void test_an_unreachable_object_is_freed(void);
void test_an_object_a_root_reaches_survives(void);
void test_a_chain_survives_through_its_root(void);
void test_a_cycle_does_not_stop_the_collector(void);
void test_an_array_element_keeps_its_target_alive(void);
void test_an_interior_pointer_keeps_its_object_alive(void);
void test_repeated_collections_reclaim_the_heap(void);

void test_a_growing_live_set_collects_a_bounded_number_of_times(void);
void test_a_grown_heap_keeps_every_value(void);
void test_a_dropped_live_set_lowers_the_trigger(void);

void test_a_global_root_keeps_an_object_alive(void);
void test_a_removed_root_array_stops_keeping_an_object(void);
void test_a_shadow_stack_slot_keeps_an_object_alive(void);
void test_a_popped_frame_stops_keeping_an_object(void);
void test_a_conservative_scan_finds_a_stack_pointer(void);
void test_a_temporary_slot_keeps_a_fresh_object_alive(void);
void test_both_root_modes_accept_the_same_program(void);
void test_a_long_jump_restores_the_shadow_stack(void);

void test_the_collector_names_itself(void);
void test_the_capabilities_agree_with_each_other(void);
void test_an_allocation_is_zeroed_and_aligned(void);
void test_the_header_records_the_type_and_the_count(void);
void test_two_allocations_never_share_a_byte(void);
void test_a_zero_length_array_yields_one_element(void);
void test_statistics_rise_with_every_allocation(void);
void test_shutdown_then_startup_gives_an_empty_heap(void);
void test_a_collection_of_an_empty_heap_is_safe(void);
void test_every_root_kind_keeps_an_object(void);
void test_a_moving_collector_refuses_a_conservative_scan(void);
void test_the_environment_variable_turns_on_torture_mode(void);

void test_a_deep_chain_survives(void);
void test_a_shared_object_stays_one_object(void);
void test_a_diamond_keeps_one_identity(void);
void test_a_self_reference_terminates(void);
void test_a_long_cycle_survives_whole(void);
void test_a_binary_tree_survives(void);
void test_a_large_object_keeps_its_field(void);
void test_a_graph_survives_repeated_collections(void);
void test_a_cleared_field_drops_its_target(void);

void test_a_non_moving_collector_keeps_every_address(void);
void test_a_moving_collector_rewrites_a_global_root(void);
void test_a_moving_collector_rewrites_a_local_through_its_slot(void);
void test_a_moving_collector_rewrites_a_temporary(void);
void test_a_moving_collector_rewrites_every_field(void);
void test_a_moving_collector_rewrites_every_array_element(void);
void test_a_moving_collector_refuses_an_interior_pointer(void);
void test_a_move_copies_every_byte(void);
void test_repeated_moves_stay_consistent(void);

void test_random_churn_keeps_every_reachable_value(void);
void test_the_live_count_matches_the_graph(void);
void test_churn_over_mixed_sizes(void);
void test_a_small_live_set_keeps_the_heap_small(void);
void test_torture_mode_survives_a_long_build(void);

void test_the_barrier_performs_the_store(void);
void test_the_barrier_stores_a_null(void);
void test_an_old_object_keeps_a_young_one_alive(void);
void test_many_old_to_young_pointers_hold(void);
void test_a_cleared_field_through_the_barrier_drops_its_target(void);
void test_the_capability_says_whether_a_barrier_is_needed(void);

void test_attach_and_detach_change_the_count(void);
void test_a_collection_waits_for_every_thread(void);
void test_a_safe_thread_does_not_stop_a_collection(void);
void test_a_nested_safe_call_keeps_the_thread_safe(void);
void test_two_threads_can_ask_for_a_collection(void);
void test_torture_mode_collects_at_every_allocation(void);

int main(void) {
    printf("collector: %s\n", lark_gc_name());

    RUN(test_alloc_returns_zeroed_payload);
    RUN(test_alloc_records_the_type);
    RUN(test_alloc_array_reserves_every_element);
    RUN(test_two_allocations_do_not_overlap);
    RUN(test_a_large_object_gets_a_page_of_its_own);
    RUN(test_statistics_count_every_allocation);
    RUN(test_many_allocations_of_every_size);
    RUN(test_an_interface_table_finds_its_method_table);

    RUN(test_base_of_finds_an_object_from_its_own_address);
    RUN(test_base_of_finds_an_object_from_an_interior_address);
    RUN(test_base_of_works_inside_a_large_object);
    RUN(test_base_of_returns_null_outside_the_heap);
    RUN(test_base_of_returns_null_for_a_freed_object);

    RUN(test_an_unreachable_object_is_freed);
    RUN(test_an_object_a_root_reaches_survives);
    RUN(test_a_chain_survives_through_its_root);
    RUN(test_a_cycle_does_not_stop_the_collector);
    RUN(test_an_array_element_keeps_its_target_alive);
    RUN(test_an_interior_pointer_keeps_its_object_alive);
    RUN(test_repeated_collections_reclaim_the_heap);

    RUN(test_a_growing_live_set_collects_a_bounded_number_of_times);
    RUN(test_a_grown_heap_keeps_every_value);
    RUN(test_a_dropped_live_set_lowers_the_trigger);

    RUN(test_a_global_root_keeps_an_object_alive);
    RUN(test_a_removed_root_array_stops_keeping_an_object);
    RUN(test_a_shadow_stack_slot_keeps_an_object_alive);
    RUN(test_a_popped_frame_stops_keeping_an_object);
    RUN(test_a_conservative_scan_finds_a_stack_pointer);
    RUN(test_a_temporary_slot_keeps_a_fresh_object_alive);
    RUN(test_both_root_modes_accept_the_same_program);
    RUN(test_a_long_jump_restores_the_shadow_stack);

    RUN(test_the_collector_names_itself);
    RUN(test_the_capabilities_agree_with_each_other);
    RUN(test_an_allocation_is_zeroed_and_aligned);
    RUN(test_the_header_records_the_type_and_the_count);
    RUN(test_two_allocations_never_share_a_byte);
    RUN(test_a_zero_length_array_yields_one_element);
    RUN(test_statistics_rise_with_every_allocation);
    RUN(test_shutdown_then_startup_gives_an_empty_heap);
    RUN(test_a_collection_of_an_empty_heap_is_safe);
    RUN(test_every_root_kind_keeps_an_object);
    RUN(test_a_moving_collector_refuses_a_conservative_scan);
    RUN(test_the_environment_variable_turns_on_torture_mode);

    RUN(test_a_deep_chain_survives);
    RUN(test_a_shared_object_stays_one_object);
    RUN(test_a_diamond_keeps_one_identity);
    RUN(test_a_self_reference_terminates);
    RUN(test_a_long_cycle_survives_whole);
    RUN(test_a_binary_tree_survives);
    RUN(test_a_large_object_keeps_its_field);
    RUN(test_a_graph_survives_repeated_collections);
    RUN(test_a_cleared_field_drops_its_target);

    RUN(test_a_non_moving_collector_keeps_every_address);
    RUN(test_a_moving_collector_rewrites_a_global_root);
    RUN(test_a_moving_collector_rewrites_a_local_through_its_slot);
    RUN(test_a_moving_collector_rewrites_a_temporary);
    RUN(test_a_moving_collector_rewrites_every_field);
    RUN(test_a_moving_collector_rewrites_every_array_element);
    RUN(test_a_moving_collector_refuses_an_interior_pointer);
    RUN(test_a_move_copies_every_byte);
    RUN(test_repeated_moves_stay_consistent);

    RUN(test_random_churn_keeps_every_reachable_value);
    RUN(test_the_live_count_matches_the_graph);
    RUN(test_churn_over_mixed_sizes);
    RUN(test_a_small_live_set_keeps_the_heap_small);
    RUN(test_torture_mode_survives_a_long_build);

    RUN(test_the_barrier_performs_the_store);
    RUN(test_the_barrier_stores_a_null);
    RUN(test_an_old_object_keeps_a_young_one_alive);
    RUN(test_many_old_to_young_pointers_hold);
    RUN(test_a_cleared_field_through_the_barrier_drops_its_target);
    RUN(test_the_capability_says_whether_a_barrier_is_needed);

    RUN(test_attach_and_detach_change_the_count);
    RUN(test_a_collection_waits_for_every_thread);
    RUN(test_a_safe_thread_does_not_stop_a_collection);
    RUN(test_a_nested_safe_call_keeps_the_thread_safe);
    RUN(test_two_threads_can_ask_for_a_collection);
    RUN(test_torture_mode_collects_at_every_allocation);

    return lark_test_report();
}
