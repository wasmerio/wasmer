#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#if defined __has_include
#if __has_include(<wasix/call_dynamic.h>)
#include <wasix/call_dynamic.h>
#define HAVE_WASIX_CALL_DYNAMIC 1
#endif
#endif

static int fib(int n) {
  if (n <= 1) {
    return n;
  }

  return fib(n - 1) + fib(n - 2);
}

static void write_i32(uint8_t *buffer, int32_t value) {
  memcpy(buffer, &value, sizeof(value));
}

static int32_t read_i32(const uint8_t *buffer) {
  int32_t value = 0;
  memcpy(&value, buffer, sizeof(value));
  return value;
}

int main() {
  printf("=== Testing direct wasix_call_dynamic ===\n");

#ifdef HAVE_WASIX_CALL_DYNAMIC
  uint8_t argument_bytes[4] = {0};
  uint8_t result_bytes[4] = {0};

  write_i32(argument_bytes, 11);
  int error = wasix_call_dynamic((wasix_function_pointer_t)fib, argument_bytes,
                                 sizeof(argument_bytes), result_bytes,
                                 sizeof(result_bytes), true);
  assert(error == 0);
  assert(read_i32(result_bytes) == 89);

  errno = 0;
  error = wasix_call_dynamic((wasix_function_pointer_t)fib, NULL, 0,
                             result_bytes, sizeof(result_bytes), true);
  assert(error == -1);
  assert(errno == EINVAL);

  memset(result_bytes, 0, sizeof(result_bytes));
  errno = 0;
  error = wasix_call_dynamic((wasix_function_pointer_t)fib, NULL, 0,
                             result_bytes, sizeof(result_bytes), false);
  assert(error == 0);
  assert(read_i32(result_bytes) == 0);

  uint8_t oversized_argument_bytes[5] = {0};
  write_i32(oversized_argument_bytes, 11);
  oversized_argument_bytes[4] = 0x5a;

  memset(result_bytes, 0, sizeof(result_bytes));
  errno = 0;
  error = wasix_call_dynamic((wasix_function_pointer_t)fib,
                             oversized_argument_bytes,
                             sizeof(oversized_argument_bytes), result_bytes,
                             sizeof(result_bytes), false);
  assert(error == 0);
  assert(read_i32(result_bytes) == 89);
#else
  assert(fib(11) == 89);
#endif

  printf("Direct dynamic call test completed\n");
  return 0;
}
