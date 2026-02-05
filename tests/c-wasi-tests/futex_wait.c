#include <assert.h>
#include <pthread.h>
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api_wasix.h>

#define TIMEOUT_NS 50000000ULL

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
    __wasi_errno_t err =
        __wasi_futex_wait((uint32_t *)a->futex, a->expected, &a->timeout, &woken);
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

static void test_mismatch_returns_woken(void)
{
    printf("Test 1: mismatch returns woken immediately\n");
    futex_word = 1;
    __wasi_bool_t woken = __WASI_BOOL_FALSE;
    __wasi_option_timestamp_t timeout = make_none();
    __wasi_errno_t err = __wasi_futex_wait((uint32_t *)&futex_word, 2, &timeout, &woken);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(woken == __WASI_BOOL_TRUE);
}

static void test_timeout_returns_not_woken(void)
{
    printf("Test 2: timeout returns not woken\n");
    futex_word = 1;
    __wasi_bool_t woken = __WASI_BOOL_TRUE;
    __wasi_option_timestamp_t timeout = make_some(TIMEOUT_NS);
    __wasi_errno_t err = __wasi_futex_wait((uint32_t *)&futex_word, 1, &timeout, &woken);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(woken == __WASI_BOOL_FALSE);
}

static void test_wake_wakes_waiter(void)
{
    printf("Test 3: wake wakes waiter\n");
    pthread_t t;
    volatile int ready = 0;
    __wasi_bool_t woken_out = __WASI_BOOL_FALSE;
    waiter_args_t args = {
        .futex = &futex_word,
        .expected = 1,
        .timeout = make_some(1000000000ULL),
        .woken_out = &woken_out,
        .ready_count = &ready,
    };

    futex_word = 1;
    assert(pthread_create(&t, 0, waiter_thread, &args) == 0);
    wait_until_ready(&ready, 1);

    __wasi_bool_t woken = __WASI_BOOL_FALSE;
    __wasi_errno_t err = __wasi_futex_wake((uint32_t *)&futex_word, &woken);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(woken == __WASI_BOOL_TRUE);

    assert(pthread_join(t, 0) == 0);
    assert(woken_out == __WASI_BOOL_TRUE);
}

static void test_invalid_futex_pointer(void)
{
    printf("Test 4: invalid futex pointer\n");
    __wasi_bool_t woken = __WASI_BOOL_FALSE;
    __wasi_option_timestamp_t timeout = make_none();
    __wasi_errno_t err = __wasi_futex_wait((uint32_t *)0xFFFFFFFF, 1, &timeout, &woken);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

static void test_invalid_woken_pointer(void)
{
    printf("Test 5: invalid woken pointer\n");
    futex_word = 1;
    __wasi_option_timestamp_t timeout = make_some(TIMEOUT_NS);
    __wasi_errno_t err =
        __wasi_futex_wait((uint32_t *)&futex_word, 1, &timeout, (__wasi_bool_t *)0xFFFFFFFF);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    test_mismatch_returns_woken();
    test_timeout_returns_not_woken();
    test_wake_wakes_waiter();
    test_invalid_futex_pointer();
    test_invalid_woken_pointer();
    printf("All tests passed!\n");
    return 0;
}
