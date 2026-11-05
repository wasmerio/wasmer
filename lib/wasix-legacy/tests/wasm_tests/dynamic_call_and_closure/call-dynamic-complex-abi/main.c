#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <wasix/call_dynamic.h>
#include <wasix/reflection.h>

typedef struct {
  int x;
  double y;
  char z[32];
} TestStruct;

static TestStruct update_struct(TestStruct s) {
  s.x += 1;
  s.y *= 2.0;
  strcat(s.z, "_updated");
  return s;
}

static int sum_ten(int a, int b, int c, int d, int e, int f, int g, int h,
                   int i, int j) {
  return a + b + c + d + e + f + g + h + i + j;
}

static TestStruct create_struct(int x, double y, const char* z) {
  TestStruct result = {0};
  result.x = x;
  result.y = y;
  strncpy(result.z, z, sizeof(result.z) - 1);
  return result;
}

static void write_i32(uint8_t** buffer, int32_t value) {
  memcpy(*buffer, &value, sizeof(value));
  *buffer += sizeof(value);
}

static void write_f64(uint8_t** buffer, double value) {
  memcpy(*buffer, &value, sizeof(value));
  *buffer += sizeof(value);
}

static void write_pointer(uint8_t** buffer, const void* ptr) {
  uint32_t value = (uint32_t)(uintptr_t)ptr;
  memcpy(*buffer, &value, sizeof(value));
  *buffer += sizeof(value);
}

static int32_t read_i32(const uint8_t* buffer) {
  int32_t value = 0;
  memcpy(&value, buffer, sizeof(value));
  return value;
}

static void assert_signature(wasix_function_pointer_t function,
                             const wasix_value_type_t* expected_args,
                             uint16_t expected_arg_count,
                             const wasix_value_type_t* expected_results,
                             uint16_t expected_result_count) {
  wasix_value_type_t actual_args[16] = {0};
  wasix_value_type_t actual_results[4] = {0};
  wasix_reflection_result_t reflection = {0};

  int error = wasix_reflect_signature(function, actual_args,
                                      sizeof(actual_args), actual_results,
                                      sizeof(actual_results), &reflection);
  assert(error == 0);
  assert(reflection.arguments == expected_arg_count);
  assert(reflection.results == expected_result_count);

  for (uint16_t i = 0; i < expected_arg_count; ++i) {
    assert(actual_args[i] == expected_args[i]);
  }
  for (uint16_t i = 0; i < expected_result_count; ++i) {
    assert(actual_results[i] == expected_results[i]);
  }
}

int main() {
  printf("=== Testing complex direct wasix_call_dynamic cases ===\n");
  {
    wasix_value_type_t expected_args[10] = {
        WASIX_VALUE_TYPE_I32, WASIX_VALUE_TYPE_I32, WASIX_VALUE_TYPE_I32,
        WASIX_VALUE_TYPE_I32, WASIX_VALUE_TYPE_I32, WASIX_VALUE_TYPE_I32,
        WASIX_VALUE_TYPE_I32, WASIX_VALUE_TYPE_I32, WASIX_VALUE_TYPE_I32,
        WASIX_VALUE_TYPE_I32,
    };
    wasix_value_type_t expected_results[1] = {WASIX_VALUE_TYPE_I32};
    assert_signature((wasix_function_pointer_t)sum_ten, expected_args, 10,
                     expected_results, 1);

    uint8_t argument_bytes[10 * sizeof(int32_t)] = {0};
    uint8_t* current = argument_bytes;
    for (int i = 1; i <= 10; ++i) {
      write_i32(&current, i);
    }

    uint8_t result_bytes[sizeof(int32_t)] = {0};
    int error = wasix_call_dynamic((wasix_function_pointer_t)sum_ten,
                                   argument_bytes, sizeof(argument_bytes),
                                   result_bytes, sizeof(result_bytes), true);
    assert(error == 0);
    assert(read_i32(result_bytes) == 55);
  }

  {
    wasix_value_type_t expected_args[2] = {WASIX_VALUE_TYPE_I32,
                                           WASIX_VALUE_TYPE_I32};
    assert_signature((wasix_function_pointer_t)update_struct, expected_args, 2,
                     NULL, 0);

    TestStruct input = {0};
    TestStruct output = {0};
    input.x = 42;
    input.y = 3.14;
    strncpy(input.z, "test_string", sizeof(input.z) - 1);

    uint8_t argument_bytes[2 * sizeof(uint32_t)] = {0};
    uint8_t* current = argument_bytes;
    write_pointer(&current, &output);
    write_pointer(&current, &input);

    int error = wasix_call_dynamic((wasix_function_pointer_t)update_struct,
                                   argument_bytes, sizeof(argument_bytes), NULL,
                                   0, true);
    assert(error == 0);
    assert(output.x == 43);
    assert(output.y > 6.2799 && output.y < 6.2801);
    assert(strcmp(output.z, "test_string_updated") == 0);
  }

  {
    wasix_value_type_t expected_args[4] = {
        WASIX_VALUE_TYPE_I32, WASIX_VALUE_TYPE_I32, WASIX_VALUE_TYPE_F64,
        WASIX_VALUE_TYPE_I32};
    assert_signature((wasix_function_pointer_t)create_struct, expected_args, 4,
                     NULL, 0);

    TestStruct output = {0};
    const char* text = "created_by_call_dynamic";
    uint8_t argument_bytes[sizeof(uint32_t) + sizeof(int32_t) + sizeof(double) +
                           sizeof(uint32_t)] = {0};
    uint8_t* current = argument_bytes;
    write_pointer(&current, &output);
    write_i32(&current, 100);
    write_f64(&current, 3.14159);
    write_pointer(&current, text);

    int error = wasix_call_dynamic((wasix_function_pointer_t)create_struct,
                                   argument_bytes, sizeof(argument_bytes), NULL,
                                   0, true);
    assert(error == 0);
    assert(output.x == 100);
    assert(output.y > 3.14158 && output.y < 3.14160);
    assert(strcmp(output.z, "created_by_call_dynamic") == 0);
  }

  printf("Complex direct dynamic call test completed\n");
  return 0;
}
