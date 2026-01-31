#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasi/api_wasix.h>

static void test_proc_signals_get_basic(void)
{
    printf("Test 1: proc_signals_get basic output\n");
    __wasi_size_t count = 0;
    __wasi_errno_t err = __wasi_proc_signals_sizes_get(&count);
    assert(err == __WASI_ERRNO_SUCCESS);

    size_t alloc_count = (count == 0) ? 1 : (size_t)count;
    __wasi_signal_disposition_t *buf =
        (__wasi_signal_disposition_t *)calloc(alloc_count + 1, sizeof(*buf));
    assert(buf != NULL);

    buf[alloc_count].sig = (__wasi_signal_t)0xAA;
    buf[alloc_count].disp = (__wasi_disposition_t)0xBB;

    err = __wasi_proc_signals_get((uint8_t *)buf);
    assert(err == __WASI_ERRNO_SUCCESS);

    for (size_t i = 0; i < (size_t)count; ++i) {
        assert(buf[i].sig <= __WASI_SIGNAL_SYS);
        assert(buf[i].disp == __WASI_DISPOSITION_DEFAULT ||
               buf[i].disp == __WASI_DISPOSITION_IGNORE);
    }

    for (size_t i = 0; i < (size_t)count; ++i) {
        for (size_t j = i + 1; j < (size_t)count; ++j) {
            assert(buf[i].sig != buf[j].sig);
        }
    }

    assert(buf[alloc_count].sig == (__wasi_signal_t)0xAA);
    assert(buf[alloc_count].disp == (__wasi_disposition_t)0xBB);

    free(buf);
}

static void test_proc_signals_get_bad_ptr(void)
{
    printf("Test 2: proc_signals_get invalid pointer\n");
    __wasi_size_t count = 0;
    __wasi_errno_t err = __wasi_proc_signals_sizes_get(&count);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint8_t *bad_ptr = (uint8_t *)(uintptr_t)0xFFFFFFFCu;
    err = __wasi_proc_signals_get(bad_ptr);
    if (count == 0) {
        assert(err == __WASI_ERRNO_SUCCESS);
    } else {
        assert(err == __WASI_ERRNO_MEMVIOLATION);
    }
}

int main(void)
{
    test_proc_signals_get_basic();
    test_proc_signals_get_bad_ptr();
    printf("All tests passed!\n");
    return 0;
}
