#include <assert.h>
#include <stdint.h>
#include <stdio.h>

#include <wasi/api_wasi.h>
#include <wasi/api_wasix.h>

static __wasi_timestamp_t now_ns(void)
{
    __wasi_timestamp_t t = 0;
    __wasi_errno_t err =
        __wasi_clock_time_get(__WASI_CLOCKID_MONOTONIC, 1, &t);
    assert(err == __WASI_ERRNO_SUCCESS);
    return t;
}

static void test_zero_duration(void)
{
    printf("Test 1: zero duration\n");
    __wasi_timestamp_t start = now_ns();
    __wasi_errno_t err = __wasi_thread_sleep(0);
    assert(err == __WASI_ERRNO_SUCCESS);
    __wasi_timestamp_t elapsed = now_ns() - start;
    assert(elapsed < 200000000ULL); /* < 200ms */
}

static void test_small_sleep(void)
{
    printf("Test 2: small sleep\n");
    const __wasi_timestamp_t dur = 10ULL * 1000ULL * 1000ULL; /* 10ms */
    __wasi_timestamp_t start = now_ns();
    __wasi_errno_t err = __wasi_thread_sleep(dur);
    assert(err == __WASI_ERRNO_SUCCESS);
    __wasi_timestamp_t elapsed = now_ns() - start;
    assert(elapsed >= 5ULL * 1000ULL * 1000ULL); /* at least 5ms */
    assert(elapsed < 1000000000ULL);             /* < 1s */
}

static void test_multiple_sleeps(void)
{
    printf("Test 3: multiple sleeps\n");
    const __wasi_timestamp_t dur = 5ULL * 1000ULL * 1000ULL; /* 5ms */
    __wasi_timestamp_t start = now_ns();
    __wasi_errno_t err = __wasi_thread_sleep(dur);
    assert(err == __WASI_ERRNO_SUCCESS);
    err = __wasi_thread_sleep(dur);
    assert(err == __WASI_ERRNO_SUCCESS);
    __wasi_timestamp_t elapsed = now_ns() - start;
    assert(elapsed >= 8ULL * 1000ULL * 1000ULL); /* at least 8ms total */
    assert(elapsed < 1000000000ULL);             /* < 1s */
}

int main(void)
{
    test_zero_duration();
    test_small_sleep();
    test_multiple_sleeps();
    printf("All tests passed!\n");
    return 0;
}
