#include <stdio.h>
#if defined __has_include
#if __has_include(<wasix/reflection.h>) && __has_include(<wasix/closure.h>)
#include <assert.h>
#include <errno.h>
#include <wasix/reflection.h>
#endif
#endif

void nothing(void) {
    // Nothing
};

int triple_add(int a, int b, int c) {
    // Nothing
};

int main() {
#if defined __has_include
#if __has_include(<wasix/reflection.h>) && __has_include(<wasix/closure.h>)
    void (*nothing_ptr)(void) = nothing;
    int (*triple_add_ptr)(int, int, int) = triple_add;
    
    wasix_reflection_result_t result;
    int code;
        
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)nothing_ptr,
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

    wasix_value_type_t params[5];
    wasix_value_type_t results[5];

    // Test that a function without parameters and results works
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)triple_add_ptr,
        params,
        5,
        results,
        5,
        &result
    );

    assert(code == 0);
    assert(errno == 0);
    assert(result.arguments == 3);
    assert(result.results == 1);
    assert(result.cacheable == 1);

    // Test that EOVERFLOW works
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)triple_add_ptr,
        params,
        2,
        results,
        1,
        &result
    );

    assert(code == -1);
    assert(errno == EOVERFLOW);
    assert(result.arguments == 3);
    assert(result.results == 1);
    assert(result.cacheable == 1);

    // Test that null pointers for params and results work if len is 0
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)triple_add_ptr,
        NULL,
        0,
        NULL,
        0,
        &result
    );

    assert(code == -1);
    assert(errno == EOVERFLOW);
    assert(result.arguments == 3);
    assert(result.results == 1);
    assert(result.cacheable == 1);

    // Test an out of bounds pointer is not cacheable
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)9999, // Out of bound, can not be cached
        NULL,
        0,
        NULL,
        0,
        &result
    );

    assert(code == -1);
    assert(errno == EINVAL);
    assert(result.arguments == 0);
    assert(result.results == 0);
    assert(result.cacheable == 0);

    // Test that a the result of the null pointer is cacheable
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)NULL, // Out of bound, can not be cached
        NULL,
        0,
        NULL,
        0,
        &result
    );

    assert(code == -1);
    assert(errno == EINVAL);
    assert(result.arguments == 0);
    assert(result.results == 0);
    assert(result.cacheable == 1);
#endif
#endif

    printf("Reflection API seems to work\n");
    return 0;
}