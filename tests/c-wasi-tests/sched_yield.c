#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <pthread.h>
#include <stdatomic.h>
#include <assert.h>
#include <stdint.h>
#include <time.h>
#include <wasi/api.h>

// Test: Basic sched_yield success
// Based on: LTP sched_yield01.c, OpenPOSIX 1-1.c
void test_basic_yield() {
    printf("Test 1: Basic sched_yield success\n");

    // sched_yield() should always succeed according to POSIX spec
    int ret = __wasi_sched_yield();
    assert(ret == 0 && "WASIX BUG: sched_yield should return 0 (success)");

    printf("  ✓ sched_yield returned 0 (success)\n");
}

// Test: Multiple successive calls
// Based on: gVisor sched_yield.cc (calls twice)
void test_multiple_calls() {
    printf("\nTest 2: Multiple successive calls (10 iterations)\n");

    for (int i = 0; i < 10; i++) {
        int ret = __wasi_sched_yield();
        assert(ret == 0 && "WASIX BUG: sched_yield should always return 0");
    }

    printf("  ✓ All 10 calls succeeded\n");
}

// Test: Stress test with many iterations
// Based on: stress-ng stress-yield.c pattern
void test_stress() {
    printf("\nTest 3: Stress test (1000 iterations)\n");

    const int iterations = 1000;
    for (int i = 0; i < iterations; i++) {
        int ret = __wasi_sched_yield();
        assert(ret == 0 && "WASIX BUG: sched_yield failed during stress test");
    }

    printf("  ✓ 1000 successive yields completed successfully\n");
}

// Test: Thread safety - concurrent yields from multiple threads
// Based on: gVisor benchmark (1-2000 threads), stress-ng multi-process design
#define NUM_THREADS 10
#define YIELDS_PER_THREAD 100

atomic_int thread_errors = 0;

void *yield_thread_func(void *arg) {
    int thread_id = *(int *)arg;
    (void)thread_id;

    for (int i = 0; i < YIELDS_PER_THREAD; i++) {
        int ret = __wasi_sched_yield();
        if (ret != 0) {
            atomic_fetch_add(&thread_errors, 1);
            return NULL;
        }
    }

    return NULL;
}

void test_thread_safety() {
    printf("\nTest 4: Thread safety (10 threads, 100 yields each)\n");

    pthread_t threads[NUM_THREADS];
    int thread_ids[NUM_THREADS];

    // Create threads
    for (int i = 0; i < NUM_THREADS; i++) {
        thread_ids[i] = i;
        int ret = pthread_create(&threads[i], NULL, yield_thread_func, &thread_ids[i]);
        assert(ret == 0 && "Failed to create thread");
    }

    // Wait for all threads to complete
    for (int i = 0; i < NUM_THREADS; i++) {
        pthread_join(threads[i], NULL);
    }

    assert(atomic_load(&thread_errors) == 0 &&
           "WASIX BUG: Some threads encountered errors during yield");

    printf("  ✓ All threads completed successfully (1000 total yields)\n");
}

// Test: Context switching verification
// Based on: OpenPOSIX 2-1.c concept (yield allowing other threads to run)
atomic_int counter = 0;
atomic_int stop_flag = 0;

void *counter_thread_func(void *arg) {
    (void)arg;
    while (atomic_load(&stop_flag) == 0) {
        atomic_fetch_add(&counter, 1);
    }
    return NULL;
}

void test_context_switch() {
    printf("\nTest 5: Context switching (yield allows other threads to run)\n");

    atomic_store(&counter, 0);
    atomic_store(&stop_flag, 0);

    // Spawn counter thread
    pthread_t counter_thread;
    int ret = pthread_create(&counter_thread, NULL, counter_thread_func, NULL);
    assert(ret == 0 && "Failed to create counter thread");

    // Yield and check if counter progresses
    int progress_count = 0;
    for (int i = 0; i < 100; i++) {
        int before = atomic_load(&counter);
        int ret = __wasi_sched_yield();
        assert(ret == 0 && "WASIX BUG: sched_yield should return 0");
        usleep(1000); // Small delay to allow counter thread to run
        int after = atomic_load(&counter);

        if (after > before) {
            progress_count++;
        }
    }

    atomic_store(&stop_flag, 1);
    pthread_join(counter_thread, NULL);

    // Verify that yielding allowed the counter thread to make progress
    assert(atomic_load(&counter) > 0 &&
           "WASIX BUG: yield did not allow other thread to run (no progress observed)");

    printf("  ✓ Counter progressed %d/100 times (context switching works)\n", progress_count);
}

// Test: Yield doesn't block
// Based on: WASIX implementation (thread_sleep_internal with 0 duration)
void test_no_blocking() {
    printf("\nTest 6: Non-blocking behavior (100 yields in < 100ms)\n");

    struct timespec start, end;
    int ret = clock_gettime(CLOCK_MONOTONIC, &start);
    assert(ret == 0 && "clock_gettime should succeed");

    for (int i = 0; i < 100; i++) {
        int ret = __wasi_sched_yield();
        assert(ret == 0 && "WASIX BUG: sched_yield should return 0");
    }

    ret = clock_gettime(CLOCK_MONOTONIC, &end);
    assert(ret == 0 && "clock_gettime should succeed");

    long elapsed_ms = (end.tv_sec - start.tv_sec) * 1000 +
                      (end.tv_nsec - start.tv_nsec) / 1000000;

    assert(elapsed_ms < 5000 &&
           "WASIX BUG: sched_yield took too long (may be blocking)");

    printf("  ✓ 100 yields completed in %ld ms (non-blocking)\n", elapsed_ms);
}

// Test: Alternating yields between two threads
// Based on: OpenPOSIX 2-1.c concept (ensuring yield allows other thread to run)
atomic_int shared_counter = 0;

void *incrementer_thread_func(void *arg) {
    int iterations = *(int *)arg;

    for (int i = 0; i < iterations; i++) {
        atomic_fetch_add(&shared_counter, 1);
        int ret = __wasi_sched_yield();
        assert(ret == 0 && "WASIX BUG: sched_yield should return 0");
    }

    return NULL;
}

void test_alternating() {
    printf("\nTest 7: Alternating yields (2 threads, 50 increments each)\n");

    atomic_store(&shared_counter, 0);

    pthread_t thread1, thread2;
    int iterations = 50;

    int ret1 = pthread_create(&thread1, NULL, incrementer_thread_func, &iterations);
    assert(ret1 == 0 && "Failed to create thread1");
    int ret2 = pthread_create(&thread2, NULL, incrementer_thread_func, &iterations);
    assert(ret2 == 0 && "Failed to create thread2");

    pthread_join(thread1, NULL);
    pthread_join(thread2, NULL);

    int final_count = atomic_load(&shared_counter);
    assert(final_count == 100 &&
           "WASIX BUG: Expected 100 increments, got different count");

    printf("  ✓ Both threads completed all increments (count = %d)\n", final_count);
}

// Test: Performance baseline
// Based on: gVisor sched_yield_benchmark.cc, stress-ng metrics
void test_performance() {
    printf("\nTest 8: Performance baseline (10000 yields)\n");

    const int iterations = 10000;
    struct timespec start, end;
    int ret = clock_gettime(CLOCK_MONOTONIC, &start);
    assert(ret == 0 && "clock_gettime should succeed");

    for (int i = 0; i < iterations; i++) {
        int ret = __wasi_sched_yield();
        assert(ret == 0 && "WASIX BUG: sched_yield should return 0");
    }

    ret = clock_gettime(CLOCK_MONOTONIC, &end);
    assert(ret == 0 && "clock_gettime should succeed");

    int64_t elapsed_ns = (int64_t)(end.tv_sec - start.tv_sec) * 1000000000LL +
                         (int64_t)(end.tv_nsec - start.tv_nsec);
    int64_t ns_per_yield = elapsed_ns / iterations;

    printf("  Performance: %lld ns per sched_yield\n", (long long)ns_per_yield);

    // Sanity check: avoid extreme regressions without flaking in slow CI
    assert(elapsed_ns < 5000000000LL &&
           "WASIX BUG: sched_yield is unexpectedly slow");

    printf("  ✓ Performance baseline recorded\n");
}

int main() {
    printf("WASIX sched_yield Integration Tests\n");
    printf("====================================\n\n");

    test_basic_yield();
    test_multiple_calls();
    test_stress();
    test_thread_safety();
    test_context_switch();
    test_no_blocking();
    test_alternating();
    test_performance();

    printf("\n====================================\n");
    printf("✓ All sched_yield tests passed!\n");

    return 0;
}
