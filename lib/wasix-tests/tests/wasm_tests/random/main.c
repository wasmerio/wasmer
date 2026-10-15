//#ExpectedStdout: ok
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <wasi/api_wasi.h>

int main(void) {
  uint8_t buffer[16384];
  memset(buffer, 0xAA, sizeof(buffer));

  __wasi_errno_t err = __wasi_random_get(buffer, sizeof(buffer));
  assert(err == __WASI_ERRNO_SUCCESS);

  size_t unchanged = 0;
  for (size_t i = 0; i < sizeof(buffer); ++i) {
    if (buffer[i] == 0xAA) {
      ++unchanged;
    }
  }

  assert(unchanged < sizeof(buffer));
  assert(__wasi_random_get(buffer, 0) == __WASI_ERRNO_SUCCESS);

  puts("ok");
  return 0;
}
