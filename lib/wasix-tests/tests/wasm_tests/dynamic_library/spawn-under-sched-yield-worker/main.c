//#ExpectedStdout: topology spawn-under-sched-yield-worker ok
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <unistd.h>
#include <wasi/api.h>

#define BUSY_THREADS 4
#define SPAWN_ROUNDS 24
#define THREADS_PER_ROUND 16

static atomic_int stop_busy = 0;

static void* sched_yield_busy(void* arg) {
  (void)arg;
  while (!atomic_load(&stop_busy)) {
    (void)__wasi_sched_yield();
    usleep(800);
  }
  return NULL;
}

static void* noop_thread(void* arg) {
  (void)arg;
  return NULL;
}

int main(void) {
  pthread_t busy[BUSY_THREADS];
  for (int i = 0; i < BUSY_THREADS; i++) {
    if (pthread_create(&busy[i], NULL, sched_yield_busy, NULL) != 0) return 1;
  }

  for (int r = 0; r < SPAWN_ROUNDS; r++) {
    pthread_t spawned[THREADS_PER_ROUND];
    for (int i = 0; i < THREADS_PER_ROUND; i++) {
      if (pthread_create(&spawned[i], NULL, noop_thread, NULL) != 0) return 2;
    }
    for (int i = 0; i < THREADS_PER_ROUND; i++) {
      pthread_join(spawned[i], NULL);
    }
  }

  atomic_store(&stop_busy, 1);
  for (int i = 0; i < BUSY_THREADS; i++) pthread_join(busy[i], NULL);

  printf("topology spawn-under-sched-yield-worker ok\n");
  fflush(stdout);
  return 0;
}
