#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/closure.h>

static void exit_with_code(uint8_t *values, uint8_t *results, void *user_data_ptr) {
  int exit_code = *(int *)user_data_ptr;
  printf("Closure call\n");
  exit(exit_code);
}

int main() {
  wasix_function_pointer_t closure_pointer = 0;
  int exit_code = 99;

  int error = wasix_closure_allocate(&closure_pointer);
  assert(error == 0);

  error = wasix_closure_prepare((wasix_function_pointer_t)exit_with_code,
                                closure_pointer, NULL, 0, NULL, 0,
                                &exit_code);
  assert(error == 0);

  ((void (*)(void))closure_pointer)();
  return 1;
}
