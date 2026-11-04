#include <dlfcn.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef NUM_SIDES
#error "NUM_SIDES must be defined by build.sh"
#endif

#ifndef SPAWN_BATCH
#error "SPAWN_BATCH must be defined by build.sh"
#endif

#ifndef SPAWN_ROUNDS
#error "SPAWN_ROUNDS must be defined by build.sh"
#endif

static atomic_int stop_pressure = 0;

static void* noop_thread(void* arg) {
  (void)arg;
  return NULL;
}

static void* malloc_pressure(void* arg) {
  (void)arg;
  while (!atomic_load(&stop_pressure)) {
    void* p = aligned_alloc(16, 64);
    if (p) {
      *((volatile char*)p) = 1;
      free(p);
    }
    p = malloc(256);
    if (p) {
      *((volatile char*)p) = 1;
      free(p);
    }
  }
  return NULL;
}

static int preload_tls_sides(void) {
  for (int i = 0; i < NUM_SIDES; i++) {
    char path[32];
    char sym[32];
    snprintf(path, sizeof(path), "./libside_%d.so", i);
    snprintf(sym, sizeof(sym), "side_touch_%d", i);

    void* handle = dlopen(path, RTLD_NOW | RTLD_GLOBAL);
    if (!handle) {
      fprintf(stderr, "dlopen %s failed: %s\n", path, dlerror());
      return 1;
    }

    typedef int (*touch_fn)(void);
    touch_fn touch = (touch_fn)dlsym(handle, sym);
    if (!touch) {
      fprintf(stderr, "dlsym %s failed: %s\n", sym, dlerror());
      return 1;
    }
    if (touch() == 0) {
      fprintf(stderr, "side_touch_%d returned zero tls base\n", i);
      return 1;
    }
  }
  return 0;
}

int main(void) {
  if (preload_tls_sides() != 0) {
    return 1;
  }

  pthread_t pressure;
  if (pthread_create(&pressure, NULL, malloc_pressure, NULL) != 0) {
    fprintf(stderr, "failed to start malloc pressure thread\n");
    return 2;
  }

  for (int round = 0; round < SPAWN_ROUNDS; round++) {
    pthread_t spawned[SPAWN_BATCH];
    for (int i = 0; i < SPAWN_BATCH; i++) {
      if (pthread_create(&spawned[i], NULL, noop_thread, NULL) != 0) {
        atomic_store(&stop_pressure, 1);
        pthread_join(pressure, NULL);
        fprintf(stderr, "pthread_create failed at round %d thread %d\n", round,
                i);
        return 3;
      }
    }
    for (int i = 0; i < SPAWN_BATCH; i++) {
      if (pthread_join(spawned[i], NULL) != 0) {
        atomic_store(&stop_pressure, 1);
        pthread_join(pressure, NULL);
        fprintf(stderr, "pthread_join failed at round %d thread %d\n", round,
                i);
        return 4;
      }
    }
  }

  atomic_store(&stop_pressure, 1);
  if (pthread_join(pressure, NULL) != 0) {
    fprintf(stderr, "failed to join malloc pressure thread\n");
    return 5;
  }

  printf("topology dl-spawn-tls-init-under-contention ok\n");
  fflush(stdout);
  return 0;
}
