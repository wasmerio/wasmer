#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#define DL_ITERATIONS 72
#define SPAWN_ITERATIONS 96

static int foo = 42;

static void* noop_thread(void* arg) {
  (void)arg;
  printf("noop thread: %d\n", foo);
  return NULL;
}

static void* spawn_loop(void* arg) {
  (void)arg;
  printf("spawn loop: %d\n", foo);

  pthread_t t;
  printf("before pthread_create\n");
  // if (pthread_create(&t, NULL, noop_thread, NULL) != 0) abort();
  // printf("after pthread_create\n");
  // pthread_join(t, NULL);
  usleep(30);

  return NULL;
}

int main(void) {
  pthread_t b;
  printf("before pthread_create from main\n");
  if (pthread_create(&b, NULL, spawn_loop, NULL) != 0) return 2;
  printf("before joining\n");
  pthread_join(b, NULL);

  printf("main done\n");
  fflush(stdout);
  return 0;
}
