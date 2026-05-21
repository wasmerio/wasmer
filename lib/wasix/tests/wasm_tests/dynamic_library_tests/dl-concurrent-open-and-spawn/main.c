#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#define DL_ITERATIONS 72
#define SPAWN_ITERATIONS 100

static void* noop_thread(void* arg) {
  (void)arg;
  return NULL;
}

static void* dlopen_loop(void* arg) {
  (void)arg;
    usleep(50000);
  for (int i = 0; i < DL_ITERATIONS; i++) {
    printf("before dlopen\n");
    void* h = dlopen("./libside.so", RTLD_NOW | RTLD_GLOBAL);
    if (h) {
      int (*f)(int) = (int (*)(int))dlsym(h, "side_func");
      if (!f || f(5) != 10) abort();
      dlclose(h);
    }
    printf("after dlopen\n");
    usleep(40);
  }
  return NULL;
}

static void* spawn_loop(void* arg) {
  (void)arg;
  for (int i = 0; i < SPAWN_ITERATIONS; i++) {
    pthread_t t;
    printf("new thread\n");
    if (pthread_create(&t, NULL, noop_thread, NULL) != 0) abort();
    printf("thread done\n");
    pthread_join(t, NULL);
    usleep(30);
  }
  return NULL;
}

int main(void) {
  pthread_t a;
  pthread_t b;
  if (pthread_create(&b, NULL, spawn_loop, NULL) != 0) return 2;
  if (pthread_create(&a, NULL, dlopen_loop, NULL) != 0) return 1;

  pthread_join(a, NULL);
  pthread_join(b, NULL);

  printf("topology dl-concurrent-open-and-spawn ok\n");
  fflush(stdout);
  return 0;
}
