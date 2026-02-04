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

static __wasi_function_pointer_t prepare_closure(uint32_t user_data)
{
    __wasi_function_pointer_t closure_id = 0;
    assert(__wasi_closure_allocate(&closure_id) == __WASI_ERRNO_SUCCESS);

    __wasi_wasm_value_type_t arg_types[2] = {
        __WASI_WASM_VALUE_TYPE_I32,
        __WASI_WASM_VALUE_TYPE_I64,
    };
    __wasi_wasm_value_type_t res_types[1] = {
        __WASI_WASM_VALUE_TYPE_I32,
    };

    __wasi_errno_t err = __wasi_closure_prepare(
        backing_function_id(),
        closure_id,
        arg_types,
        2,
        res_types,
        1,
        (uint8_t *)&user_data);
    assert(err == __WASI_ERRNO_SUCCESS);

    return closure_id;
}

static void reset_backing_state(void)
{
    backing_calls = 0;
    seen_a = 0;
    seen_b = 0;
    seen_user = 0;
}

static void test_strict_success(void)
{
    printf("Test 1: strict call_dynamic success\n");

    uint32_t user_data = 7;
    __wasi_function_pointer_t closure_id = prepare_closure(user_data);

    uint32_t a = 5;
    uint64_t b = 9;
    uint8_t values[12];
    uint8_t results[4] = {0};
    memcpy(values, &a, sizeof(a));
    memcpy(values + sizeof(a), &b, sizeof(b));

    reset_backing_state();
    __wasi_errno_t err = __wasi_call_dynamic(
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

static void test_strict_values_len_too_short(void)
{
    printf("Test 2: strict values too short -> EINVAL\n");

    uint32_t user_data = 1;
    __wasi_function_pointer_t closure_id = prepare_closure(user_data);

    uint32_t a = 11;
    uint8_t values[4];
    uint8_t results[4] = {0};
    memcpy(values, &a, sizeof(a));

    __wasi_errno_t err = __wasi_call_dynamic(
        closure_id,
        values,
        sizeof(values),
        results,
        sizeof(results),
        __WASI_BOOL_TRUE);
    assert(err == __WASI_ERRNO_INVAL);

    assert(__wasi_closure_free(closure_id) == __WASI_ERRNO_SUCCESS);
}

static void test_strict_values_len_too_long(void)
{
    printf("Test 3: strict values too long -> EINVAL\n");

    uint32_t user_data = 2;
    __wasi_function_pointer_t closure_id = prepare_closure(user_data);

    uint32_t a = 1;
    uint64_t b = 2;
    uint8_t values[16];
    uint8_t results[4] = {0};
    memcpy(values, &a, sizeof(a));
    memcpy(values + sizeof(a), &b, sizeof(b));
    memset(values + 12, 0xCC, 4);

    __wasi_errno_t err = __wasi_call_dynamic(
        closure_id,
        values,
        sizeof(values),
        results,
        sizeof(results),
        __WASI_BOOL_TRUE);
    assert(err == __WASI_ERRNO_INVAL);

    assert(__wasi_closure_free(closure_id) == __WASI_ERRNO_SUCCESS);
}

static void test_strict_results_len_too_short(void)
{
    printf("Test 4: strict results too short -> EINVAL\n");

    uint32_t user_data = 3;
    __wasi_function_pointer_t closure_id = prepare_closure(user_data);

    uint32_t a = 10;
    uint64_t b = 20;
    uint8_t values[12];
    uint8_t results[1] = {0};
    memcpy(values, &a, sizeof(a));
    memcpy(values + sizeof(a), &b, sizeof(b));

    __wasi_errno_t err = __wasi_call_dynamic(
        closure_id,
        values,
        sizeof(values),
        results,
        sizeof(results),
        __WASI_BOOL_TRUE);
    assert(err == __WASI_ERRNO_INVAL);

    assert(__wasi_closure_free(closure_id) == __WASI_ERRNO_SUCCESS);
}

static void test_non_strict_defaults(void)
{
    printf("Test 5: non-strict defaults missing values\n");

    uint32_t user_data = 4;
    __wasi_function_pointer_t closure_id = prepare_closure(user_data);

    uint32_t a = 7;
    uint8_t values[4];
    uint8_t results[4] = {0};
    memcpy(values, &a, sizeof(a));

    reset_backing_state();
    __wasi_errno_t err = __wasi_call_dynamic(
        closure_id,
        values,
        sizeof(values),
        results,
        sizeof(results),
        __WASI_BOOL_FALSE);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint32_t out = 0;
    memcpy(&out, results, sizeof(out));

    assert(backing_calls == 1);
    assert(seen_a == a);
    assert(seen_b == 0);
    assert(seen_user == user_data);
    assert(out == (uint32_t)(a + user_data));

    assert(__wasi_closure_free(closure_id) == __WASI_ERRNO_SUCCESS);
}

static void test_non_strict_extra_values(void)
{
    printf("Test 6: non-strict ignores extra values\n");

    uint32_t user_data = 5;
    __wasi_function_pointer_t closure_id = prepare_closure(user_data);

    uint32_t a = 3;
    uint64_t b = 4;
    uint8_t values[20];
    uint8_t results[4] = {0};
    memcpy(values, &a, sizeof(a));
    memcpy(values + sizeof(a), &b, sizeof(b));
    memset(values + 12, 0xAB, 8);

    reset_backing_state();
    __wasi_errno_t err = __wasi_call_dynamic(
        closure_id,
        values,
        sizeof(values),
        results,
        sizeof(results),
        __WASI_BOOL_FALSE);
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

static void test_non_strict_results_len_zero(void)
{
    printf("Test 7: non-strict results too short succeeds\n");

    uint32_t user_data = 6;
    __wasi_function_pointer_t closure_id = prepare_closure(user_data);

    uint32_t a = 1;
    uint64_t b = 2;
    uint8_t values[12];
    uint8_t results[4];
    memcpy(values, &a, sizeof(a));
    memcpy(values + sizeof(a), &b, sizeof(b));
    memset(results, 0xAA, sizeof(results));

    reset_backing_state();
    __wasi_errno_t err = __wasi_call_dynamic(
        closure_id,
        values,
        sizeof(values),
        results,
        0,
        __WASI_BOOL_FALSE);
    assert(err == __WASI_ERRNO_SUCCESS);

    assert(backing_calls == 1);
    assert(results[0] == 0xAA);
    assert(results[1] == 0xAA);
    assert(results[2] == 0xAA);
    assert(results[3] == 0xAA);

    assert(__wasi_closure_free(closure_id) == __WASI_ERRNO_SUCCESS);
}

static void test_invalid_pointer(void)
{
    printf("Test 8: invalid pointer returns MEMVIOLATION\n");

    uint32_t user_data = 8;
    __wasi_function_pointer_t closure_id = prepare_closure(user_data);

    const uint8_t *bad_ptr = (const uint8_t *)0xFFFFFFFFu;
    uint8_t results[4] = {0};

    __wasi_errno_t err = __wasi_call_dynamic(
        closure_id,
        bad_ptr,
        4,
        results,
        sizeof(results),
        __WASI_BOOL_FALSE);
    assert(err == __WASI_ERRNO_MEMVIOLATION);

    assert(__wasi_closure_free(closure_id) == __WASI_ERRNO_SUCCESS);
}

int main(void)
{
    printf("WASIX call_dynamic integration tests\n");
    test_strict_success();
    test_strict_values_len_too_short();
    test_strict_values_len_too_long();
    test_strict_results_len_too_short();
    test_non_strict_defaults();
    test_non_strict_extra_values();
    test_non_strict_results_len_zero();
    test_invalid_pointer();
    printf("All tests passed!\n");
    return 0;
}
