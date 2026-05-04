#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <wasix/closure.h>
#include <wasix/function_pointer.h>
#include <wasix/reflection.h>
#include <wasix/value_type.h>

static void closure_backing_function(uint8_t* values, uint8_t* results,
                                     void* user_data_ptr) {}

static wasix_function_pointer_t closure_backing_function_id(void) {
  void (*fn)(uint8_t*, uint8_t*, void*) = closure_backing_function;
  return (wasix_function_pointer_t)(uintptr_t)fn;
}

static void flush_trace(void) { fflush(stdout); }

int main() {
  wasix_function_pointer_t closure_pointer = 0;
  wasix_value_type_t argument_types[2] = {WASIX_VALUE_TYPE_I32,
                                          WASIX_VALUE_TYPE_I64};
  wasix_value_type_t result_types[1] = {WASIX_VALUE_TYPE_I32};
  wasix_reflection_result_t result = {0};
  wasix_value_type_t actual_arguments[2] = {0};
  wasix_value_type_t actual_results[1] = {0};
  printf("allocating closure\n");
  flush_trace();
  int code = wasix_closure_allocate(&closure_pointer);
  printf("closure_allocate rc=%d closure_nonzero=%d\n", code,
         closure_pointer != 0);
  flush_trace();
  assert(code == 0);
  assert(closure_pointer != 0);

  printf("preparing closure with 2 args and 1 result\n");
  flush_trace();
  code = wasix_closure_prepare(closure_backing_function_id(), closure_pointer,
                               argument_types, 2, result_types, 1, NULL);
  printf("closure_prepare rc=%d\n", code);
  flush_trace();
  assert(code == 0);

  printf("reflecting closure signature\n");
  flush_trace();
  code = wasix_reflect_signature(closure_pointer, actual_arguments, 2,
                                 actual_results, 1, &result);
  printf(
      "reflect_signature rc=%d errno=%d cacheable=%d args=%zu results=%zu "
      "arg0=%d arg1=%d result0=%d\n",
      code, errno, result.cacheable, result.arguments, result.results,
      actual_arguments[0], actual_arguments[1], actual_results[0]);
  flush_trace();
  assert(code == 0);
  assert(result.cacheable == 0);
  assert(result.arguments == 2);
  assert(result.results == 1);
  assert(actual_arguments[0] == WASIX_VALUE_TYPE_I32);
  assert(actual_arguments[1] == WASIX_VALUE_TYPE_I64);
  assert(actual_results[0] == WASIX_VALUE_TYPE_I32);

  code = wasix_closure_free(closure_pointer);
  printf("closure_free rc=%d\n", code);
  flush_trace();
  assert(code == 0);

  printf("Reflection API seems to work with closures\n");
  return 0;
}