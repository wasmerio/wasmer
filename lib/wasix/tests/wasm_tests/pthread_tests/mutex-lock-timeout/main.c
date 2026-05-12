#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

void* f(void* arg) {
  (void)arg;

  struct timespec ts;
  if (clock_gettime(CLOCK_REALTIME, &ts) != 0) {
    perror("clock_gettime");
    return (void*)(intptr_t)errno;
  }

  ts.tv_nsec += 500 * 1000 * 1000;  // 0.5 seconds
  if (ts.tv_nsec >= 1000000000) {
    ts.tv_sec += 1;
    ts.tv_nsec -= 1000000000;
  }

  return (void*)(intptr_t)pthread_mutex_timedlock(&lock, &ts);
}

int main(void) {
  int rc = pthread_mutex_lock(&lock);
  if (rc != 0) {
    fprintf(stderr, "pthread_mutex_lock: %s\n", strerror(rc));
    return 1;
  }

  pthread_t thread;
  rc = pthread_create(&thread, NULL, f, NULL);
  if (rc != 0) {
    fprintf(stderr, "pthread_create: %s\n", strerror(rc));
    pthread_mutex_unlock(&lock);
    return 1;
  }

  void* thread_result;
  rc = pthread_join(thread, &thread_result);
  if (rc != 0) {
    fprintf(stderr, "pthread_join: %s\n", strerror(rc));
    pthread_mutex_unlock(&lock);
    return 1;
  }

  pthread_mutex_unlock(&lock);

  int timedlock_rc = (int)(intptr_t)thread_result;
  if (timedlock_rc != ETIMEDOUT) {
    fprintf(stderr, "pthread_mutex_timedlock returned %d, expected %d\n",
            timedlock_rc, ETIMEDOUT);
    return 1;
  }

  return 0;
}
