#include <dlfcn.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#define SPAWN_ROUNDS 256

static atomic_int stop_dlsym = 0;
static atomic_int worker_failed = 0;

static void* noop_thread(void* arg) {
  (void)arg;
  return NULL;
}

static void* dlsym_worker(void* arg) {
  (void)arg;
  void* handle = dlopen("./libside.so", RTLD_NOW | RTLD_GLOBAL);
  if (!handle) {
    atomic_store(&worker_failed, 1);
    atomic_store(&stop_dlsym, 1);
    return NULL;
  }

  while (!atomic_load(&stop_dlsym)) {
    int (*fn)(int) = (int (*)(int))dlsym(handle, "side_func");
    if (!fn) {
      atomic_store(&worker_failed, 1);
      atomic_store(&stop_dlsym, 1);
      break;
    }
    if (fn(11) != 22) {
      atomic_store(&stop_dlsym, 1);
      dlclose(handle);
      return NULL;
    }
  }
  dlclose(handle);
  return NULL;
}

int main(void) {
  pthread_t sym;
  if (pthread_create(&sym, NULL, dlsym_worker, NULL) != 0) return 1;

  for (int i = 0; i < SPAWN_ROUNDS; i++) {
    pthread_t t;
    if (pthread_create(&t, NULL, noop_thread, NULL) != 0) {
      atomic_store(&stop_dlsym, 1);
      pthread_join(sym, NULL);
      return 2;
    }
    pthread_join(t, NULL);
  }

  atomic_store(&stop_dlsym, 1);
  pthread_join(sym, NULL);

  if (atomic_load(&worker_failed)) return 3;

  printf("topology dl-spawn-under-dlsym-worker ok\n");
  fflush(stdout);
  return 0;
}
