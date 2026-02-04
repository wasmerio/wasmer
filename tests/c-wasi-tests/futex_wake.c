#include <assert.h>
#include <pthread.h>
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api_wasix.h>

#define TIMEOUT_NS 1000000000ULL

static _Alignas(4) volatile uint32_t futex_word = 1;

static __wasi_option_timestamp_t make_none(void)
{
    __wasi_option_timestamp_t t;
    memset(&t, 0, sizeof(t));
    t.tag = __WASI_OPTION_NONE;
    t.u.none = 0;
    return t;
}

static __wasi_option_timestamp_t make_some(__wasi_timestamp_t ns)
{
    __wasi_option_timestamp_t t;
    memset(&t, 0, sizeof(t));
    t.tag = __WASI_OPTION_SOME;
    t.u.some = ns;
    return t;
}

typedef struct {
    volatile uint32_t *futex;
    uint32_t expected;
    __wasi_option_timestamp_t timeout;
    __wasi_bool_t *woken_out;
    volatile int *ready_count;
} waiter_args_t;

static void *waiter_thread(void *arg)
{
    waiter_args_t *a = (waiter_args_t *)arg;
    __sync_fetch_and_add(a->ready_count, 1);

    __wasi_bool_t woken = __WASI_BOOL_FALSE;
    __wasi_errno_t err = __wasi_futex_wait((uint32_t *)a->futex, a->expected, &a->timeout, &woken);
    assert(err == __WASI_ERRNO_SUCCESS);

    *a->woken_out = woken;
    return 0;
}

static void wait_until_ready(volatile int *ready, int target)
{
    for (int i = 0; i < 1000000 && *ready < target; i++) {
        sched_yield();
    }
    assert(*ready == target);
    usleep(1000);
}

static void test_no_waiters(void)
{
    printf("Test 1: wake with no waiters returns false\n");
    __wasi_bool_t woken = __WASI_BOOL_TRUE;
    __wasi_errno_t err = __wasi_futex_wake((uint32_t *)&futex_word, &woken);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(woken == __WASI_BOOL_FALSE);
}

static void test_single_waiter_woken(void)
{
    printf("Test 2: single waiter wakes\n");
    pthread_t t;
    volatile int ready = 0;
    __wasi_bool_t woken_out = __WASI_BOOL_FALSE;
    waiter_args_t args = {
        .futex = &futex_word,
        .expected = 1,
        .timeout = make_some(TIMEOUT_NS),
        .woken_out = &woken_out,
        .ready_count = &ready,
    };

    assert(pthread_create(&t, 0, waiter_thread, &args) == 0);
    wait_until_ready(&ready, 1);

    __wasi_bool_t woken = __WASI_BOOL_FALSE;
    __wasi_errno_t err = __wasi_futex_wake((uint32_t *)&futex_word, &woken);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(woken == __WASI_BOOL_TRUE);

    assert(pthread_join(t, 0) == 0);
    assert(woken_out == __WASI_BOOL_TRUE);
}

static void test_wake_all(void)
{
    printf("Test 3: wake_all wakes all waiters\n");
    enum { kThreads = 4 };
    pthread_t threads[kThreads];
    waiter_args_t args[kThreads];
    __wasi_bool_t woken_out[kThreads];
    volatile int ready = 0;

    for (int i = 0; i < kThreads; i++) {
        woken_out[i] = __WASI_BOOL_FALSE;
        args[i] = (waiter_args_t){
            .futex = &futex_word,
            .expected = 1,
            .timeout = make_some(TIMEOUT_NS),
            .woken_out = &woken_out[i],
            .ready_count = &ready,
        };
        assert(pthread_create(&threads[i], 0, waiter_thread, &args[i]) == 0);
    }

    wait_until_ready(&ready, kThreads);

    __wasi_bool_t woken = __WASI_BOOL_FALSE;
    __wasi_errno_t err = __wasi_futex_wake_all((uint32_t *)&futex_word, &woken);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(woken == __WASI_BOOL_TRUE);

    for (int i = 0; i < kThreads; i++) {
        assert(pthread_join(threads[i], 0) == 0);
        assert(woken_out[i] == __WASI_BOOL_TRUE);
    }
}

static void test_wake_some(void)
{
    printf("Test 4: wake some waiters\n");
    enum { kThreads = 5, kWake = 3 };
    pthread_t threads[kThreads];
    waiter_args_t args[kThreads];
    __wasi_bool_t woken_out[kThreads];
    volatile int ready = 0;

    for (int i = 0; i < kThreads; i++) {
        woken_out[i] = __WASI_BOOL_FALSE;
        args[i] = (waiter_args_t){
            .futex = &futex_word,
            .expected = 1,
            .timeout = make_some(TIMEOUT_NS),
            .woken_out = &woken_out[i],
            .ready_count = &ready,
        };
        assert(pthread_create(&threads[i], 0, waiter_thread, &args[i]) == 0);
    }

    wait_until_ready(&ready, kThreads);

    for (int i = 0; i < kWake; i++) {
        __wasi_bool_t woken = __WASI_BOOL_FALSE;
        __wasi_errno_t err = __wasi_futex_wake((uint32_t *)&futex_word, &woken);
        assert(err == __WASI_ERRNO_SUCCESS);
        assert(woken == __WASI_BOOL_TRUE);
    }

    int woken_count = 0;
    int timed_out = 0;
    for (int i = 0; i < kThreads; i++) {
        assert(pthread_join(threads[i], 0) == 0);
        if (woken_out[i] == __WASI_BOOL_TRUE) {
            woken_count++;
        } else {
            timed_out++;
        }
    }

    assert(woken_count == kWake);
    assert(timed_out == (kThreads - kWake));
}

static void test_no_waiters_after_drained(void)
{
    printf("Test 5: wake after draining returns false\n");
    __wasi_bool_t woken = __WASI_BOOL_TRUE;
    __wasi_errno_t err = __wasi_futex_wake((uint32_t *)&futex_word, &woken);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(woken == __WASI_BOOL_FALSE);
}

int main(void)
{
    test_no_waiters();
    test_single_waiter_woken();
    test_wake_all();
    test_wake_some();
    test_no_waiters_after_drained();
    printf("All tests passed!\n");
    return 0;
}
