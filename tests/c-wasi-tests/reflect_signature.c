#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <wasix/closure.h>
#include <wasix/function_pointer.h>
#include <wasix/reflection.h>
#include <wasix/value_type.h>

static int32_t test_signature(int32_t a, int64_t b, float c, double d)
{
    return (int32_t)(a + (int32_t)b + (int32_t)c + (int32_t)d);
}

static void closure_backing(uint8_t *values, uint8_t *results, void *user_data)
{
    (void)values;
    (void)results;
    (void)user_data;
}

static wasix_function_pointer_t function_id_of_test_signature(void)
{
    int32_t (*fn)(int32_t, int64_t, float, double) = test_signature;
    return (wasix_function_pointer_t)(uintptr_t)fn;
}

static wasix_function_pointer_t function_id_of_closure_backing(void)
{
    void (*fn)(uint8_t *, uint8_t *, void *) = closure_backing;
    return (wasix_function_pointer_t)(uintptr_t)fn;
}

static void test_basic_signature(void)
{
    printf("Test 1: basic signature reflection\n");
    wasix_function_pointer_t fn_id = function_id_of_test_signature();

    wasix_value_type_t args[4] = {0};
    wasix_value_type_t results[1] = {0};
    wasix_reflection_result_t info = {0};

    int rc = wasix_reflect_signature(fn_id, args, 4, results, 1, &info);
    assert(rc == 0);
    assert(info.cacheable == __WASI_BOOL_TRUE);
    assert(info.arguments == 4);
    assert(info.results == 1);

    assert(args[0] == WASIX_VALUE_TYPE_I32);
    assert(args[1] == WASIX_VALUE_TYPE_I64);
    assert(args[2] == WASIX_VALUE_TYPE_F32);
    assert(args[3] == WASIX_VALUE_TYPE_F64);
    assert(results[0] == WASIX_VALUE_TYPE_I32);
}

static void test_extra_buffer_unchanged(void)
{
    printf("Test 2: extra buffer bytes remain unchanged\n");
    wasix_function_pointer_t fn_id = function_id_of_test_signature();

    wasix_value_type_t args[6];
    wasix_value_type_t results[3];
    memset(args, 0xAA, sizeof(args));
    memset(results, 0xBB, sizeof(results));

    wasix_reflection_result_t info = {0};
    int rc = wasix_reflect_signature(fn_id, args, 6, results, 3, &info);
    assert(rc == 0);
    assert(info.arguments == 4);
    assert(info.results == 1);

    assert(args[0] == WASIX_VALUE_TYPE_I32);
    assert(args[1] == WASIX_VALUE_TYPE_I64);
    assert(args[2] == WASIX_VALUE_TYPE_F32);
    assert(args[3] == WASIX_VALUE_TYPE_F64);
    assert(args[4] == (wasix_value_type_t)0xAA);
    assert(args[5] == (wasix_value_type_t)0xAA);

    assert(results[0] == WASIX_VALUE_TYPE_I32);
    assert(results[1] == (wasix_value_type_t)0xBB);
    assert(results[2] == (wasix_value_type_t)0xBB);
}

static void test_overflow_arguments(void)
{
    printf("Test 3: overflow on arguments buffer\n");
    wasix_function_pointer_t fn_id = function_id_of_test_signature();

    wasix_value_type_t args[4];
    wasix_value_type_t results[1];
    memset(args, 0xCC, sizeof(args));
    memset(results, 0xDD, sizeof(results));

    wasix_reflection_result_t info = {0};
    errno = 0;
    int rc = wasix_reflect_signature(fn_id, args, 1, results, 1, &info);
    assert(rc == -1);
    assert(errno == EOVERFLOW);
    assert(info.arguments == 4);
    assert(info.results == 1);

    for (size_t i = 0; i < 4; i++) {
        assert(args[i] == (wasix_value_type_t)0xCC);
    }
    assert(results[0] == (wasix_value_type_t)0xDD);
}

static void test_overflow_results(void)
{
    printf("Test 4: overflow on results buffer\n");
    wasix_function_pointer_t fn_id = function_id_of_test_signature();

    wasix_value_type_t args[4];
    memset(args, 0xEE, sizeof(args));

    wasix_reflection_result_t info = {0};
    errno = 0;
    int rc = wasix_reflect_signature(fn_id, args, 4, NULL, 0, &info);
    assert(rc == -1);
    assert(errno == EOVERFLOW);
    assert(info.arguments == 4);
    assert(info.results == 1);

    for (size_t i = 0; i < 4; i++) {
        assert(args[i] == (wasix_value_type_t)0xEE);
    }
}

static void test_invalid_function_id_zero(void)
{
    printf("Test 5: invalid function id (zero)\n");
    wasix_value_type_t args[1] = {0};
    wasix_value_type_t results[1] = {0};
    wasix_reflection_result_t info = {0};

    errno = 0;
    int rc = wasix_reflect_signature(0, args, 1, results, 1, &info);
    assert(rc == -1);
    assert(errno == EINVAL);
    assert(info.cacheable == __WASI_BOOL_TRUE);
    assert(info.arguments == 0);
    assert(info.results == 0);
}

static void test_invalid_function_id_oob(void)
{
    printf("Test 6: invalid function id (out of bounds)\n");
    wasix_value_type_t args[1] = {0};
    wasix_value_type_t results[1] = {0};
    wasix_reflection_result_t info = {0};

    errno = 0;
    int rc = wasix_reflect_signature(0xFFFFFFFFu, args, 1, results, 1, &info);
    assert(rc == -1);
    assert(errno == EINVAL);
    assert(info.cacheable == __WASI_BOOL_FALSE);
    assert(info.arguments == 0);
    assert(info.results == 0);
}

static void test_null_result_pointer(void)
{
    printf("Test 7: null result pointer\n");
    wasix_function_pointer_t fn_id = function_id_of_test_signature();
    wasix_value_type_t args[4] = {0};
    wasix_value_type_t results[1] = {0};

    errno = 0;
    int rc = wasix_reflect_signature(fn_id, args, 4, results, 1, NULL);
    assert(rc == -1);
    assert(errno == EMEMVIOLATION);
}

static void test_closure_cacheable_flag(void)
{
    printf("Test 8: closure cacheable flag\n");
    wasix_function_pointer_t backing_id = function_id_of_closure_backing();
    wasix_function_pointer_t closure_id = 0;

    int rc = wasix_closure_allocate(&closure_id);
    assert(rc == 0);

    wasix_value_type_t arg_types[2] = {WASIX_VALUE_TYPE_I32, WASIX_VALUE_TYPE_I64};
    wasix_value_type_t res_types[1] = {WASIX_VALUE_TYPE_I32};

    rc = wasix_closure_prepare(
        backing_id,
        closure_id,
        arg_types,
        2,
        res_types,
        1,
        NULL);
    assert(rc == 0);

    wasix_value_type_t args_out[2] = {0};
    wasix_value_type_t results_out[1] = {0};
    wasix_reflection_result_t info = {0};
    rc = wasix_reflect_signature(closure_id, args_out, 2, results_out, 1, &info);
    assert(rc == 0);
    assert(info.cacheable == __WASI_BOOL_FALSE);
    assert(info.arguments == 2);
    assert(info.results == 1);
    assert(args_out[0] == WASIX_VALUE_TYPE_I32);
    assert(args_out[1] == WASIX_VALUE_TYPE_I64);
    assert(results_out[0] == WASIX_VALUE_TYPE_I32);

    rc = wasix_closure_free(closure_id);
    assert(rc == 0);
}

int main(void)
{
    test_basic_signature();
    test_extra_buffer_unchanged();
    test_overflow_arguments();
    test_overflow_results();
    test_invalid_function_id_zero();
    test_invalid_function_id_oob();
    test_null_result_pointer();
    test_closure_cacheable_flag();
    printf("All tests passed!\n");
    return 0;
}
