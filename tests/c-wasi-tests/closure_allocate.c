#include <assert.h>
#include <stdint.h>
#include <stdio.h>

#include <wasi/api_wasix.h>

static void test_basic_allocate_free(void)
{
    printf("Test 1: closure_allocate + closure_free\n");

    __wasi_function_pointer_t closure_index = 0;
    __wasi_errno_t err = __wasi_closure_allocate(&closure_index);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(closure_index != 0);

    err = __wasi_closure_free(closure_index);
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_multiple_allocations_unique(void)
{
    printf("Test 2: multiple allocations are unique\n");

    __wasi_function_pointer_t a = 0;
    __wasi_function_pointer_t b = 0;
    __wasi_function_pointer_t c = 0;

    assert(__wasi_closure_allocate(&a) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_closure_allocate(&b) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_closure_allocate(&c) == __WASI_ERRNO_SUCCESS);

    assert(a != b);
    assert(a != c);
    assert(b != c);

    assert(__wasi_closure_free(a) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_closure_free(b) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_closure_free(c) == __WASI_ERRNO_SUCCESS);
}

static void test_invalid_pointer(void)
{
    printf("Test 3: closure_allocate invalid pointer\n");

    __wasi_function_pointer_t *bad_ptr =
        (__wasi_function_pointer_t *)(uintptr_t)0xFFFFFFFCu;
    __wasi_errno_t err = __wasi_closure_allocate(bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    printf("WASIX closure_allocate integration tests\n");
    test_basic_allocate_free();
    test_multiple_allocations_unique();
    test_invalid_pointer();
    printf("All tests passed!\n");
    return 0;
}
