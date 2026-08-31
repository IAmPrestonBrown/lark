/* Threads, the stop the world protocol, and the foreign call states. */

#include <pthread.h>
#include <stdatomic.h>
#include <string.h>

#include "lark_test.h"
#include "test_types.h"

static void *worker_globals[1];

/* covers: M-24 */
void test_attach_and_detach_change_the_count(void);
void test_attach_and_detach_change_the_count(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    CHECK(lark_thread_count() == 1u);
    /* A second attach on the same thread does nothing. */
    lark_thread_attach();
    CHECK(lark_thread_count() == 1u);
    lark_shutdown();
}

/* What one worker thread does. */
typedef struct worker_task {
    atomic_int allocations;
    atomic_int ready;
    atomic_int stop;
} worker_task;

static void *allocating_worker(void *argument) {
    worker_task *task = argument;
    lark_thread_attach();

    node *held = NULL;
    TEST_FRAME(frame, 1, 0);
    frame.s[0] = (void **)&held;
    frame.h.nslots = 1;

    atomic_store(&task->ready, 1);
    while (atomic_load(&task->stop) == 0) {
        held = lark_alloc(&NODE_TYPE);
        atomic_fetch_add(&task->allocations, 1);
        /* Rule M-16 puts a poll at a loop back edge. */
        LARK_POLL();
    }

    lark_frame_pop(&frame.h);
    lark_thread_detach();
    return NULL;
}

/* covers: M-23, M-25, M-26 */
void test_a_collection_waits_for_every_thread(void);
void test_a_collection_waits_for_every_thread(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);
    memset(worker_globals, 0, sizeof worker_globals);
    lark_root_register(worker_globals, 1);

    worker_task task;
    atomic_init(&task.allocations, 0);
    atomic_init(&task.ready, 0);
    atomic_init(&task.stop, 0);

    pthread_t threads[4];
    for (size_t index = 0; index < 4u; index += 1) {
        CHECK(pthread_create(&threads[index], NULL, allocating_worker, &task) == 0);
    }
    while (atomic_load(&task.ready) == 0) {
        /* Wait for at least one worker to register. */
    }

    for (int round = 0; round < 20; round += 1) {
        lark_collect();
    }
    /* Every explicit call collected. A collector whose allocation triggers a
     * collection adds more, because the workers allocate throughout. */
    CHECK(lark_gc_statistics().collections >= 20u);

    atomic_store(&task.stop, 1);
    /* Rule M-19. A join blocks outside Lark, so the thread enters the safe
     * state first. A collector that waits for the world would otherwise wait
     * for a thread that never reaches a safepoint again. */
    lark_enter_safe();
    for (size_t index = 0; index < 4u; index += 1) {
        CHECK(pthread_join(threads[index], NULL) == 0);
    }
    lark_leave_safe();
    CHECK(atomic_load(&task.allocations) > 0);
    CHECK(lark_thread_count() == 1u);

    lark_root_remove(worker_globals);
    lark_shutdown();
}

/* What one thread that sits in a foreign call does. */
typedef struct safe_task {
    atomic_int entered;
    atomic_int leave;
} safe_task;

static void *safe_worker(void *argument) {
    safe_task *task = argument;
    lark_thread_attach();
    /* Rule M-19. The thread enters the safe state before a foreign call. */
    lark_enter_safe();
    atomic_store(&task->entered, 1);
    while (atomic_load(&task->leave) == 0) {
        /* Stay in the foreign call. */
    }
    lark_leave_safe();
    lark_thread_detach();
    return NULL;
}

/* covers: M-19, M-26 */
void test_a_safe_thread_does_not_stop_a_collection(void);
void test_a_safe_thread_does_not_stop_a_collection(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);

    safe_task task;
    atomic_init(&task.entered, 0);
    atomic_init(&task.leave, 0);

    pthread_t worker;
    REQUIRE(pthread_create(&worker, NULL, safe_worker, &task) == 0);
    while (atomic_load(&task.entered) == 0) {
        /* Wait for the worker to reach the safe state. */
    }

    /* The worker never polls, and the collection still finishes. */
    lark_collect();
    CHECK(lark_gc_statistics().collections == 1u);

    atomic_store(&task.leave, 1);
    /* Rule M-19. A join blocks outside Lark, so the thread enters the safe
     * state first. */
    lark_enter_safe();
    CHECK(pthread_join(worker, NULL) == 0);
    lark_leave_safe();
    lark_shutdown();
}

/* covers: M-19 */
void test_a_nested_safe_call_keeps_the_thread_safe(void);
void test_a_nested_safe_call_keeps_the_thread_safe(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);

    safe_task task;
    atomic_init(&task.entered, 0);
    atomic_init(&task.leave, 0);

    /* One foreign call can hold another, as in `f(g())`. The inner pair must
     * not return the thread to the running state. */
    lark_enter_safe();
    lark_enter_safe();
    lark_leave_safe();

    pthread_t worker;
    REQUIRE(pthread_create(&worker, NULL, safe_worker, &task) == 0);
    while (atomic_load(&task.entered) == 0) {
        /* Wait for the worker to reach the safe state. */
    }
    atomic_store(&task.leave, 1);
    /* Rule M-19. A join blocks outside Lark, so the thread enters the safe
     * state first. */
    lark_enter_safe();
    CHECK(pthread_join(worker, NULL) == 0);
    lark_leave_safe();

    lark_leave_safe();
    CHECK(lark_thread_count() == 1u);
    lark_shutdown();
}

/* covers: M-26 */
void test_two_threads_can_ask_for_a_collection(void);
void test_two_threads_can_ask_for_a_collection(void) {
    test_start(LARK_ROOTS_SHADOW_STACK);

    worker_task task;
    atomic_init(&task.allocations, 0);
    atomic_init(&task.ready, 0);
    atomic_init(&task.stop, 0);

    pthread_t worker;
    REQUIRE(pthread_create(&worker, NULL, allocating_worker, &task) == 0);
    while (atomic_load(&task.ready) == 0) {
        /* Wait for the worker to register. */
    }

    for (int round = 0; round < 10; round += 1) {
        lark_collect();
    }

    atomic_store(&task.stop, 1);
    /* Rule M-19. A join blocks outside Lark, so the thread enters the safe
     * state first. */
    lark_enter_safe();
    CHECK(pthread_join(worker, NULL) == 0);
    lark_leave_safe();
    CHECK(lark_gc_statistics().collections >= 10u);
    lark_shutdown();
}

/* covers: F-3 */
void test_torture_mode_collects_at_every_allocation(void);
void test_torture_mode_collects_at_every_allocation(void) {
    SKIP_UNLESS(lark_gc_capabilities().reclaims,
                "the collector frees nothing");
    lark_gc_config config = lark_gc_config_default();
    config.roots = LARK_ROOTS_SHADOW_STACK;
    config.torture = true;
    lark_startup_with(config);

    node *held = NULL;
    TEST_FRAME(frame, 1, 0);
    frame.s[0] = (void **)&held;
    frame.h.nslots = 1;

    for (int index = 0; index < 25; index += 1) {
        held = lark_alloc(&NODE_TYPE);
        REQUIRE(held != NULL);
        held->value = index;
        CHECK(held->value == index);
    }
    /* Rule F-3. Every allocation ran a full collection. */
    CHECK(lark_gc_statistics().collections >= 25u);
    /* One more collection, so the count is the live set rather than the live
     * set plus whatever the last allocation added after the last collection. */
    lark_collect();
    CHECK(lark_gc_statistics().live_objects == 1u);

    lark_frame_pop(&frame.h);
    lark_shutdown();
}
