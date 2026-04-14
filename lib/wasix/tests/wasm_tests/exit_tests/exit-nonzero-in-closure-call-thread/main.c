#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <wasix/closure.h>

static void exit_with_code(uint8_t *values, uint8_t *results, void *user_data_ptr) {
  int exit_code = *(int *)user_data_ptr;
  printf("Closure call in thread\n");
  exit(exit_code);
}

static void *thread_func(void *data) {
  int exit_code = 99;
  wasix_function_pointer_t closure_pointer = 0;

  int error = wasix_closure_allocate(&closure_pointer);
  assert(error == 0);

  error = wasix_closure_prepare((wasix_function_pointer_t)exit_with_code,
                                closure_pointer, NULL, 0, NULL, 0,
                                &exit_code);
  assert(error == 0);

  ((void (*)(void))closure_pointer)();
  return 0;
}

int main() {
  pthread_attr_t attr = {0};
  if (pthread_attr_init(&attr) != 0) {
    perror("init attr");
    return -1;
  }

  pthread_t thread = {0};
  if (pthread_create(&thread, &attr, &thread_func,
                     (void *)stdout
                     ) != 0) {
    perror("create thread");
    return -1;
  }

  void *thread_ret;
  if (pthread_join(thread, &thread_ret) != 0) {
    perror("join");
    return -1;
  }
  sleep(1);

  return 1;
}
