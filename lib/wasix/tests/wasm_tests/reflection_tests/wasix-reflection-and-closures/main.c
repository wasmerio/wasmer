#include <stdio.h>

#if defined __has_include
#if __has_include(<wasix/reflection.h>) && __has_include(<wasix/closure.h>)
#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <wasix/closure.h>
#include <wasix/reflection.h>
#endif
#endif

#if defined __has_include
#if __has_include(<wasix/reflection.h>) && __has_include(<wasix/closure.h>)
static void closure_backing_function(uint8_t* values, uint8_t* results,
                                     void* user_data_ptr) {}
#endif
#endif

int main() {
#if defined __has_include
#if __has_include(<wasix/reflection.h>) && __has_include(<wasix/closure.h>)
  wasix_function_pointer_t closure_pointer = 0;
  wasix_value_type_t argument_types[2] = {WASIX_VALUE_TYPE_I32,
                                          WASIX_VALUE_TYPE_I64};
  wasix_value_type_t result_types[1] = {WASIX_VALUE_TYPE_I32};
  int code = wasix_closure_allocate(&closure_pointer);
  if (code == 0) {
    code = wasix_closure_prepare(
        (wasix_function_pointer_t)closure_backing_function, closure_pointer,
        argument_types, 2, result_types, 1, NULL);
    if (code == 0) {
      wasix_reflection_result_t result = {0};
      wasix_value_type_t actual_arguments[2] = {0};
      wasix_value_type_t actual_results[1] = {0};

      code = wasix_reflect_signature((wasix_function_pointer_t)closure_pointer,
                                     actual_arguments, 2, actual_results, 1,
                                     &result);

      if (code == 0) {
        assert(result.cacheable == 0);
      }
    }

    wasix_closure_free(closure_pointer);
  }
#endif
#endif

  printf("Reflection API seems to work with closures\n");
  return 0;
}