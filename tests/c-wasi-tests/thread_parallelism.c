#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

static __wasi_size_t get_parallelism(void)
{
    __wasi_size_t n = 0;
    __wasi_errno_t err = __wasi_thread_parallelism(&n);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(n >= 1);
    return n;
}

static void test_parallelism_consistency(void)
{
    printf("Test 1: thread_parallelism consistency\n");
    __wasi_size_t n1 = get_parallelism();
    __wasi_size_t n2 = get_parallelism();
    assert(n1 == n2);
    printf("  Parallelism: %u\n", (unsigned)n1);
}

static void test_sysconf_matches(void)
{
    printf("Test 2: sysconf matches thread_parallelism\n");
    __wasi_size_t n = get_parallelism();
    long onln = sysconf(_SC_NPROCESSORS_ONLN);
    long conf = sysconf(_SC_NPROCESSORS_CONF);

    assert(onln > 0);
    assert(conf > 0);
    assert((__wasi_size_t)onln == n);
    assert((__wasi_size_t)conf == n);
}

static void test_parallelism_fault(void)
{
    printf("Test 3: thread_parallelism invalid pointer\n");
    __wasi_size_t *bad_ptr = (__wasi_size_t *)(uintptr_t)0xFFFFFFFCu;
    __wasi_errno_t err = __wasi_thread_parallelism(bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    test_parallelism_consistency();
    test_sysconf_matches();
    test_parallelism_fault();
    printf("All tests passed!\n");
    return 0;
}
