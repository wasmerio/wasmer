#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasi/api_wasix.h>

static void test_sizes_get_consistency(void)
{
    printf("Test 1: proc_signals_sizes_get consistency\n");
    __wasi_size_t count1 = 0;
    __wasi_size_t count2 = 0;

    __wasi_errno_t err = __wasi_proc_signals_sizes_get(&count1);
    assert(err == __WASI_ERRNO_SUCCESS);
    err = __wasi_proc_signals_sizes_get(&count2);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(count1 == count2);

    printf("  Signal count: %u\n", (unsigned)count1);
}

static void test_sizes_get_matches_get(void)
{
    printf("Test 2: proc_signals_get matches size\n");
    __wasi_size_t count = 0;
    __wasi_errno_t err = __wasi_proc_signals_sizes_get(&count);
    assert(err == __WASI_ERRNO_SUCCESS);

    size_t alloc_count = (count == 0) ? 1 : (size_t)count;
    __wasi_signal_disposition_t *buf =
        (__wasi_signal_disposition_t *)calloc(alloc_count, sizeof(*buf));
    assert(buf != NULL);

    err = __wasi_proc_signals_get((uint8_t *)buf);
    assert(err == __WASI_ERRNO_SUCCESS);

    for (size_t i = 0; i < (size_t)count; ++i)
    {
        assert(buf[i].sig <= __WASI_SIGNAL_SYS);
        assert(buf[i].disp == __WASI_DISPOSITION_DEFAULT ||
               buf[i].disp == __WASI_DISPOSITION_IGNORE);
    }
    for (size_t i = 0; i < (size_t)count; ++i)
    {
        for (size_t j = i + 1; j < (size_t)count; ++j)
        {
            assert(buf[i].sig != buf[j].sig);
        }
    }

    free(buf);

    __wasi_size_t count_after = 0;
    err = __wasi_proc_signals_sizes_get(&count_after);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(count == count_after);
}

static void test_sizes_get_fault(void)
{
    printf("Test 3: proc_signals_sizes_get invalid pointer\n");
    __wasi_size_t *bad_ptr = (__wasi_size_t *)(uintptr_t)0xFFFFFFFCu;
    __wasi_errno_t err = __wasi_proc_signals_sizes_get(bad_ptr);
    printf("  err=%u\n", (unsigned)err);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    test_sizes_get_consistency();
    test_sizes_get_matches_get();
    test_sizes_get_fault();
    printf("All tests passed!\n");
    return 0;
}
