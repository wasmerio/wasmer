//#ExpectedStdout: topology spawn-under-nanosleep-worker ok
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <time.h>

#define BUSY_THREADS 4
#define SPAWN_ROUNDS 24
#define THREADS_PER_ROUND 16

static atomic_int stop_busy = 0;

static void* nanosleep_busy(void* arg) {
  (void)arg;
  struct timespec ts = {.tv_sec = 0, .tv_nsec = 1500000L};
  while (!atomic_load(&stop_busy)) {
    nanosleep(&ts, NULL);
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
    if (pthread_create(&busy[i], NULL, nanosleep_busy, NULL) != 0) return 1;
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

  printf("topology spawn-under-nanosleep-worker ok\n");
  fflush(stdout);
  return 0;
}
