#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <wasix/closure.h>

typedef struct {
  int call_count;
} ClosureState;

static void write_i32(uint8_t* buffer, int32_t value) {
  memcpy(buffer, &value, sizeof(value));
}

static int32_t read_i32(const uint8_t* buffer) {
  int32_t value = 0;
  memcpy(&value, buffer, sizeof(value));
  return value;
}

static void closure_backing_function(uint8_t* values, uint8_t* results,
                                     void* user_data_ptr) {
  ClosureState* state = (ClosureState*)user_data_ptr;
  int a = read_i32(values);
  int b = read_i32(values + sizeof(int32_t));

  printf("Inside closure callback: %d + %d (called %d times)\n", a, b,
         state->call_count);

  write_i32(results, a + b + state->call_count);
  state->call_count += 1;
}

int main() {
  printf("=== Testing direct wasix closures ===\n");

  wasix_function_pointer_t closure_pointer = 0;
  wasix_value_type_t argument_types[2] = {WASIX_VALUE_TYPE_I32,
                                          WASIX_VALUE_TYPE_I32};
  wasix_value_type_t result_types[1] = {WASIX_VALUE_TYPE_I32};
  ClosureState initial_state = {.call_count = 0};

  int error = wasix_closure_allocate(&closure_pointer);
  assert(error == 0);

  error = wasix_closure_prepare(
      (wasix_function_pointer_t)closure_backing_function, closure_pointer,
      argument_types, 2, result_types, 1, &initial_state);
  assert(error == 0);

  int (*closure_func)(int, int) = (int (*)(int, int))closure_pointer;
  assert(closure_func(10, 20) == 30);
  assert(closure_func(5, 7) == 13);
  assert(closure_func(100, 200) == 302);

  ClosureState redefined_state = {.call_count = 100};
  error = wasix_closure_prepare(
      (wasix_function_pointer_t)closure_backing_function, closure_pointer,
      argument_types, 2, result_types, 1, &redefined_state);
  assert(error == 0);
  assert(closure_func(1, 2) == 103);

  error = wasix_closure_free(closure_pointer);
  assert(error == 0);

  printf("Closure test completed\n");
  return 0;
}
