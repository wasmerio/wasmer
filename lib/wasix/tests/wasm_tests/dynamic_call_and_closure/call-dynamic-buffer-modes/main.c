#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <wasix/call_dynamic.h>

static int weighted_sum(int a, int b, int c) { return a + 10 * b + 100 * c; }

static void write_i32(uint8_t* buffer, int32_t value) {
  memcpy(buffer, &value, sizeof(value));
}

static int32_t read_i32(const uint8_t* buffer) {
  int32_t value = 0;
  memcpy(&value, buffer, sizeof(value));
  return value;
}

int main() {
  uint8_t full_arguments[3 * sizeof(int32_t)] = {0};
  write_i32(full_arguments, 1);
  write_i32(full_arguments + sizeof(int32_t), 2);
  write_i32(full_arguments + 2 * sizeof(int32_t), 3);

  uint8_t exact_result[sizeof(int32_t)] = {0};
  int error = wasix_call_dynamic((wasix_function_pointer_t)weighted_sum,
                                 full_arguments, sizeof(full_arguments),
                                 exact_result, sizeof(exact_result), true);
  assert(error == 0);
  assert(read_i32(exact_result) == 321);

  uint8_t short_arguments[2 * sizeof(int32_t)] = {0};
  write_i32(short_arguments, 1);
  write_i32(short_arguments + sizeof(int32_t), 2);

  errno = 0;
  error = wasix_call_dynamic((wasix_function_pointer_t)weighted_sum,
                             short_arguments, sizeof(short_arguments),
                             exact_result, sizeof(exact_result), true);
  assert(error == -1);
  assert(errno == EINVAL);

  memset(exact_result, 0, sizeof(exact_result));
  errno = 0;
  error = wasix_call_dynamic((wasix_function_pointer_t)weighted_sum,
                             short_arguments, sizeof(short_arguments),
                             exact_result, sizeof(exact_result), false);
  assert(error == 0);
  assert(read_i32(exact_result) == 21);

  uint8_t oversized_result[2 * sizeof(int32_t)];
  memset(oversized_result, 0x7a, sizeof(oversized_result));

  errno = 0;
  error = wasix_call_dynamic((wasix_function_pointer_t)weighted_sum,
                             full_arguments, sizeof(full_arguments),
                             oversized_result, sizeof(oversized_result), true);
  assert(error == -1);
  assert(errno == EINVAL);

  memset(oversized_result, 0x7a, sizeof(oversized_result));
  errno = 0;
  error = wasix_call_dynamic((wasix_function_pointer_t)weighted_sum,
                             full_arguments, sizeof(full_arguments),
                             oversized_result, sizeof(oversized_result), false);
  assert(error == 0);
  assert(read_i32(oversized_result) == 321);
  assert(read_i32(oversized_result + sizeof(int32_t)) == 0x7a7a7a7a);

  printf("Strict vs non-strict dynamic call test completed\n");
  return 0;
}
