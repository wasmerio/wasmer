//#ExpectedExitCode:13455

#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>

#ifndef NULL
#define NULL ((void*)0)
#endif

void* abort_in_thread(void* arg) {
  abort();
  return NULL;
}

int main() {
  pthread_t thread;
  if (pthread_create(&thread, NULL, abort_in_thread, NULL) != 0) {
    perror("pthread_create");
    return 1;
  }

  pthread_join(thread, NULL);
  return 5;
}
