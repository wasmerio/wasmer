#include <assert.h>
#include <stdint.h>
#include <stdio.h>

#include <wasi/api_wasix.h>
#include <wasix/context.h>

static int context_supported = 0;

static void test_destroy_main_context(void)
{
    printf("Test 1: destroy main context\n");

    __wasi_context_id_t main_id = wasix_context_main;
    __wasi_errno_t err = __wasi_context_destroy(main_id);

    assert(err == __WASI_ERRNO_NOTSUP || err == __WASI_ERRNO_INVAL);
    context_supported = (err != __WASI_ERRNO_NOTSUP);
}

static void test_destroy_missing_context(void)
{
    printf("Test 2: destroy missing context\n");

    __wasi_context_id_t missing = 0xDEADBEEFu;
    __wasi_errno_t err = __wasi_context_destroy(missing);

    if (!context_supported) {
        assert(err == __WASI_ERRNO_NOTSUP);
        return;
    }

    assert(err == __WASI_ERRNO_SUCCESS);
}

int main(void)
{
    printf("WASIX context_destroy integration tests\n");
    test_destroy_main_context();
    test_destroy_missing_context();
    printf("All tests passed!\n");
    return 0;
}
