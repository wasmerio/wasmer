#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <wasi/api_wasi.h>

static void test_clock_time_get(void)
{
    // From wasmtime p1_clock_time_get.rs.
    printf("Test 1: clock_time_get precision and monotonicity\n");
    __wasi_timestamp_t t1 = 0;
    __wasi_timestamp_t t2 = 0;
    __wasi_errno_t err = __wasi_clock_time_get(__WASI_CLOCKID_MONOTONIC, 1, &t1);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_clock_time_get(__WASI_CLOCKID_MONOTONIC, 0, &t1);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_clock_time_get(__WASI_CLOCKID_MONOTONIC, 0, &t2);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(t1 <= t2);
}

static void test_all_clocks(void)
{
    printf("Test 2: all clocks succeed\n");
    __wasi_timestamp_t t = 0;
    __wasi_errno_t err = __wasi_clock_time_get(__WASI_CLOCKID_REALTIME, 0, &t);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(t > 0);

    err = __wasi_clock_time_get(__WASI_CLOCKID_MONOTONIC, 0, &t);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(t > 0);

    err = __wasi_clock_time_get(__WASI_CLOCKID_PROCESS_CPUTIME_ID, 0, &t);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_clock_time_get(__WASI_CLOCKID_THREAD_CPUTIME_ID, 0, &t);
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_invalid_clock(void)
{
    printf("Test 3: invalid clock id\n");
    __wasi_timestamp_t t = 0;
    __wasi_clockid_t bad = (__wasi_clockid_t)UINT32_C(0xFFFFFFFF);
    __wasi_errno_t err = __wasi_clock_time_get(bad, 0, &t);
    assert(err == __WASI_ERRNO_INVAL);
}

static void test_invalid_pointer(void)
{
    printf("Test 4: invalid pointer\n");
    __wasi_timestamp_t *bad_ptr = (__wasi_timestamp_t *)(uintptr_t)0xFFFFFFFCu;
    __wasi_errno_t err = __wasi_clock_time_get(__WASI_CLOCKID_MONOTONIC, 0, bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    test_clock_time_get();
    test_all_clocks();
    test_invalid_clock();
    test_invalid_pointer();
    printf("All tests passed!\n");
    return 0;
}
