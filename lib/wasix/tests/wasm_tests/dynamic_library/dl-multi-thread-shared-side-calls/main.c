#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>

#define WORKER_THREADS 8
#define CALLS_PER_THREAD 2000

static void* shared_handle;

static void* call_side_many(void* arg) {
  (void)arg;
  int (*f)(int) = (int (*)(int))dlsym(shared_handle, "side_func");
  if (!f) abort();
  for (int i = 0; i < CALLS_PER_THREAD; i++) {
    if (f(13 + (i & 7)) != 2 * (13 + (i & 7))) abort();
  }
  return NULL;
}

int main(void) {
  shared_handle = dlopen("./libside.so", RTLD_NOW | RTLD_GLOBAL);
  if (!shared_handle) abort();

  pthread_t workers[WORKER_THREADS];
  for (int i = 0; i < WORKER_THREADS; i++) {
    if (pthread_create(&workers[i], NULL, call_side_many, NULL) != 0) abort();
  }
  for (int i = 0; i < WORKER_THREADS; i++) pthread_join(workers[i], NULL);

  if (dlclose(shared_handle) != 0) abort();

  printf("topology dl-multi-thread-shared-side-calls ok\n");
  fflush(stdout);
  return 0;
}
