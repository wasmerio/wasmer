#include <stdio.h>

#if defined __has_include
#if __has_include(<wasix/reflection.h>)
#include <assert.h>
#include <errno.h>
#include <wasix/reflection.h>
#endif
#endif

static int static_add(int a, int b) {
    return a + b;
}

static void static_no_params(void) {
    printf("Static function with no params\n");
}

static double static_double_func(double x, double y) {
    return x * y;
}

int main() {
#if defined __has_include
#if __has_include(<wasix/reflection.h>)
    wasix_reflection_result_t result;
    int code;
    wasix_value_type_t params[5];
    wasix_value_type_t results[5];
    
    // Test reflection on static function with int parameters
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)static_add,
        params,
        5,
        results,
        5,
        &result
    );

    assert(code == 0);
    assert(errno == 0);
    assert(result.arguments == 2);
    assert(result.results == 1);
    assert(result.cacheable == 1);
    assert(params[0] == WASIX_VALUE_TYPE_I32);
    assert(params[1] == WASIX_VALUE_TYPE_I32);
    assert(results[0] == WASIX_VALUE_TYPE_I32);

    // Test reflection on static function without parameters
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)static_no_params,
        NULL,
        0,
        NULL,
        0,
        &result
    );

    assert(code == 0);
    assert(errno == 0);
    assert(result.arguments == 0);
    assert(result.results == 0);
    assert(result.cacheable == 1);

    // Test reflection on static function with double parameters
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)static_double_func,
        params,
        5,
        results,
        5,
        &result
    );

    assert(code == 0);
    assert(errno == 0);
    assert(result.arguments == 2);
    assert(result.results == 1);
    assert(result.cacheable == 1);
    assert(params[0] == WASIX_VALUE_TYPE_F64);
    assert(params[1] == WASIX_VALUE_TYPE_F64);
    assert(results[0] == WASIX_VALUE_TYPE_F64);
#endif
#endif

    printf("Reflection API works with static functions\n");
    return 0;
}
