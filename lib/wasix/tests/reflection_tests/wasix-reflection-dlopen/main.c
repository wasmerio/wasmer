#include <stdio.h>
#include <dlfcn.h>

#if defined __has_include
#if __has_include(<wasix/reflection.h>)
#include <assert.h>
#include <errno.h>
#include <wasix/reflection.h>
#endif
#endif

int main() {
#if defined __has_include
#if __has_include(<wasix/reflection.h>)
    // Open the shared library
    void* handle = dlopen("./liblibrary.so", RTLD_LAZY);
    assert(handle != NULL);

    // Get function pointers from the library
    int (*add_three_ptr)(int, int, int) = dlsym(handle, "add_three");
    assert(add_three_ptr != NULL);
    
    void (*no_params_ptr)(void) = dlsym(handle, "no_params_no_results");
    assert(no_params_ptr != NULL);

    wasix_reflection_result_t result;
    int code;
    
    // Test reflection on dlopened function with parameters
    wasix_value_type_t params[5];
    wasix_value_type_t results[5];
    
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)add_three_ptr,
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
    assert(params[0] == WASIX_VALUE_TYPE_I32);
    assert(params[1] == WASIX_VALUE_TYPE_I32);
    assert(params[2] == WASIX_VALUE_TYPE_I32);
    assert(results[0] == WASIX_VALUE_TYPE_I32);

    // Test reflection on dlopened function without parameters
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)no_params_ptr,
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

    dlclose(handle);
#endif
#endif

    printf("Reflection API works with dlopened functions\n");
    return 0;
}
