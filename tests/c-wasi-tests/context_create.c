#include <assert.h>
#include <stdint.h>
#include <stdio.h>

#include <wasi/api_wasi.h>
#include <wasi/api_wasix.h>
#include <wasix/context.h>

static int context_supported = 0;

static void entrypoint_ok(void) {
    (void)0;
}

static void entrypoint_bad(int value) {
    (void)value;
}

static __wasi_function_pointer_t fn_ptr(void (*fn)(void)) {
    return (__wasi_function_pointer_t)(uintptr_t)fn;
}

static __wasi_function_pointer_t fn_ptr_bad(void (*fn)(int)) {
    return (__wasi_function_pointer_t)(uintptr_t)fn;
}

static void test_create_basic(void)
{
    printf("Test 1: context_create basic\n");

    __wasi_context_id_t ctx_id = 0;
    __wasi_errno_t err = __wasi_context_create(&ctx_id, fn_ptr(entrypoint_ok));
    if (err == __WASI_ERRNO_NOTSUP) {
        context_supported = 0;
        return;
    }

    assert(err == __WASI_ERRNO_SUCCESS);
    context_supported = 1;
    assert(ctx_id != wasix_context_main);

    assert(__wasi_context_destroy(ctx_id) == __WASI_ERRNO_SUCCESS);
}

static void test_invalid_entrypoint_id(void)
{
    printf("Test 2: invalid entrypoint id -> EINVAL\n");

    if (!context_supported) {
        return;
    }

    __wasi_context_id_t ctx_id = 0;
    __wasi_errno_t err = __wasi_context_create(&ctx_id, 0xDEADBEEFu);
    assert(err == __WASI_ERRNO_INVAL);
}

static void test_invalid_entrypoint_signature(void)
{
    printf("Test 3: invalid entrypoint signature -> EINVAL\n");

    if (!context_supported) {
        return;
    }

    __wasi_context_id_t ctx_id = 0;
    __wasi_errno_t err = __wasi_context_create(&ctx_id, fn_ptr_bad(entrypoint_bad));
    if (err == __WASI_ERRNO_SUCCESS) {
        assert(__wasi_context_destroy(ctx_id) == __WASI_ERRNO_SUCCESS);
    }
    assert(err == __WASI_ERRNO_INVAL);
}

static void test_invalid_pointer(void)
{
    printf("Test 4: invalid context_id pointer -> MEMVIOLATION\n");

    if (!context_supported) {
        return;
    }

    __wasi_context_id_t *bad_ptr = (__wasi_context_id_t *)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_context_create(bad_ptr, fn_ptr(entrypoint_ok));
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    printf("WASIX context_create integration tests\n");
    test_create_basic();
    test_invalid_entrypoint_id();
    test_invalid_entrypoint_signature();
    test_invalid_pointer();
    printf("All tests passed!\n");
    return 0;
}
