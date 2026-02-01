#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <wasi/api_wasi.h>
#include <wasi/api_wasix.h>

static volatile int backing_calls = 0;
static volatile uint32_t seen_a = 0;
static volatile uint64_t seen_b = 0;
static volatile uint32_t seen_user = 0;

static void closure_backing(uint8_t *values, uint8_t *results, void *user_data)
{
    backing_calls++;

    uint32_t a = 0;
    uint64_t b = 0;
    uint32_t user = 0;

    memcpy(&a, values, sizeof(a));
    memcpy(&b, values + sizeof(a), sizeof(b));
    memcpy(&user, user_data, sizeof(user));

    seen_a = a;
    seen_b = b;
    seen_user = user;

    uint32_t out = a + (uint32_t)b + user;
    memcpy(results, &out, sizeof(out));
}

static __wasi_function_pointer_t backing_function_id(void)
{
    void (*fn)(uint8_t *, uint8_t *, void *) = closure_backing;
    return (__wasi_function_pointer_t)(uintptr_t)fn;
}

static void test_basic_prepare_and_call(void)
{
    printf("Test 1: closure_prepare + call_dynamic\n");

    __wasi_function_pointer_t closure_id = 0;
    assert(__wasi_closure_allocate(&closure_id) == __WASI_ERRNO_SUCCESS);

    __wasi_wasm_value_type_t arg_types[2] = {
        __WASI_WASM_VALUE_TYPE_I32,
        __WASI_WASM_VALUE_TYPE_I64,
    };
    __wasi_wasm_value_type_t res_types[1] = {
        __WASI_WASM_VALUE_TYPE_I32,
    };

    uint32_t user_data = 7;
    __wasi_errno_t err = __wasi_closure_prepare(
        backing_function_id(),
        closure_id,
        arg_types,
        2,
        res_types,
        1,
        (uint8_t *)&user_data);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint32_t a = 5;
    uint64_t b = 9;
    uint8_t values[12];
    uint8_t results[4] = {0};
    memcpy(values, &a, sizeof(a));
    memcpy(values + sizeof(a), &b, sizeof(b));

    backing_calls = 0;
    seen_a = 0;
    seen_b = 0;
    seen_user = 0;

    err = __wasi_call_dynamic(
        closure_id,
        values,
        sizeof(values),
        results,
        sizeof(results),
        __WASI_BOOL_TRUE);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint32_t out = 0;
    memcpy(&out, results, sizeof(out));

    assert(backing_calls == 1);
    assert(seen_a == a);
    assert(seen_b == b);
    assert(seen_user == user_data);
    assert(out == (uint32_t)(a + (uint32_t)b + user_data));

    assert(__wasi_closure_free(closure_id) == __WASI_ERRNO_SUCCESS);
}

static void test_invalid_type(void)
{
    printf("Test 2: invalid value type -> EINVAL\n");

    __wasi_function_pointer_t closure_id = 0;
    assert(__wasi_closure_allocate(&closure_id) == __WASI_ERRNO_SUCCESS);

    __wasi_wasm_value_type_t bad_arg = (uint8_t)0xFFu;
    __wasi_wasm_value_type_t res_types[1] = {
        __WASI_WASM_VALUE_TYPE_I32,
    };

    __wasi_errno_t err = __wasi_closure_prepare(
        backing_function_id(),
        closure_id,
        &bad_arg,
        1,
        res_types,
        1,
        NULL);
    assert(err == __WASI_ERRNO_INVAL);

    assert(__wasi_closure_free(closure_id) == __WASI_ERRNO_SUCCESS);
}

int main(void)
{
    printf("WASIX closure_prepare integration tests\n");
    test_basic_prepare_and_call();
    test_invalid_type();
    printf("All tests passed!\n");
    return 0;
}
