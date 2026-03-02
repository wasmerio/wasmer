#include <stdio.h>

#if defined __has_include
#if __has_include(<wasix/reflection.h>) && __has_include(<wasix/closure.h>)
#include <assert.h>
#include <errno.h>
#include <wasix/reflection.h>
#include <wasix/closure.h>
#endif
#endif

int main() {
#if defined __has_include
#if __has_include(<wasix/reflection.h>) && __has_include(<wasix/closure.h>)
    wasix_function_pointer_t closure_pointer;
    wasix_closure_allocate(
        (wasix_function_pointer_t*)&closure_pointer
    );

    wasix_reflection_result_t result;
    int code;
    
    // Test that closures are not cacheable
    code = wasix_reflect_signature(
        (wasix_function_pointer_t)closure_pointer,
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
#endif
#endif

    printf("Reflection API seems to work with closures\n");
    return 0;
}