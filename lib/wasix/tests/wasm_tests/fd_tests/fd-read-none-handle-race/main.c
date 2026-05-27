//#ExpectedStdout: race open/close test passed (20000 iterations)
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define ITERATIONS 20000

static pthread_barrier_t start_barrier;
static pthread_barrier_t end_barrier;
static atomic_int shared_fd;

static void fail_iteration(const char* step, int iteration, int detail) {
  fprintf(stderr, "iteration %d failed at %s (detail=%d)\n", iteration, step,
          detail);
}

static void* close_worker(void* arg) {
  (void)arg;
  for (int i = 0; i < ITERATIONS; i++) {
    int rc = pthread_barrier_wait(&start_barrier);
    if (!(rc == 0 || rc == PTHREAD_BARRIER_SERIAL_THREAD)) {
      fail_iteration("start barrier in close worker", i, rc);
      return (void*)1;
    }

    sched_yield();
    int old_fd = atomic_load_explicit(&shared_fd, memory_order_acquire);
    if (close(old_fd) != 0) {
      fail_iteration("close(old_fd)", i, errno);
      rc = pthread_barrier_wait(&end_barrier);
      if (!(rc == 0 || rc == PTHREAD_BARRIER_SERIAL_THREAD)) {
        fail_iteration("end barrier in close worker after close failure", i, rc);
      }
      return (void*)1;
    }

    rc = pthread_barrier_wait(&end_barrier);
    if (!(rc == 0 || rc == PTHREAD_BARRIER_SERIAL_THREAD)) {
      fail_iteration("end barrier in close worker", i, rc);
      return (void*)1;
    }
  }
  return NULL;
}

int main(void) {
  const char* path = "fd_read_none_handle_race_file";

  unlink(path);
  int seed_fd = open(path, O_CREAT | O_TRUNC | O_RDWR, 0644);
  if (seed_fd < 0) {
    perror("open seed file");
    return 1;
  }
  if (write(seed_fd, "x", 1) != 1) {
    perror("write seed file");
    close(seed_fd);
    return 1;
  }
  if (close(seed_fd) != 0) {
    perror("close seed file");
    return 1;
  }

  if (pthread_barrier_init(&start_barrier, NULL, 2) != 0) {
    perror("pthread_barrier_init(start)");
    return 1;
  }
  if (pthread_barrier_init(&end_barrier, NULL, 2) != 0) {
    perror("pthread_barrier_init(end)");
    return 1;
  }
  atomic_store_explicit(&shared_fd, -1, memory_order_release);

  pthread_t closer;
  if (pthread_create(&closer, NULL, close_worker, NULL) != 0) {
    perror("pthread_create(close_worker)");
    return 1;
  }

  for (int i = 0; i < ITERATIONS; i++) {
    int old_fd = open(path, O_RDONLY);
    if (old_fd < 0) {
      fail_iteration("open(old_fd)", i, errno);
      return 1;
    }
    atomic_store_explicit(&shared_fd, old_fd, memory_order_release);

    int rc = pthread_barrier_wait(&start_barrier);
    if (!(rc == 0 || rc == PTHREAD_BARRIER_SERIAL_THREAD)) {
      fail_iteration("start barrier in main", i, rc);
      return 1;
    }

    sched_yield();
    int new_fd = open(path, O_RDONLY);
    if (new_fd < 0) {
      fail_iteration("open(new_fd)", i, errno);
      return 1;
    }

    struct stat st;
    if (fstat(new_fd, &st) != 0) {
      fail_iteration("fstat(new_fd)", i, errno);
      close(new_fd);
      return 1;
    }

    char value = 0;
    errno = 0;
    ssize_t read_len = pread(new_fd, &value, 1, 0);
    if (read_len != 1) {
      fail_iteration("pread(new_fd)", i, errno);
      close(new_fd);
      return 1;
    }
    if (value != 'x') {
      fail_iteration("pread content", i, (int)value);
      close(new_fd);
      return 1;
    }

    if (close(new_fd) != 0) {
      fail_iteration("close(new_fd)", i, errno);
      return 1;
    }

    rc = pthread_barrier_wait(&end_barrier);
    if (!(rc == 0 || rc == PTHREAD_BARRIER_SERIAL_THREAD)) {
      fail_iteration("end barrier in main", i, rc);
      return 1;
    }
  }

  void* worker_ret = NULL;
  if (pthread_join(closer, &worker_ret) != 0) {
    perror("pthread_join(close_worker)");
    return 1;
  }
  if (worker_ret != NULL) {
    fprintf(stderr, "close worker failed\n");
    return 1;
  }

  if (pthread_barrier_destroy(&start_barrier) != 0) {
    perror("pthread_barrier_destroy(start)");
    return 1;
  }
  if (pthread_barrier_destroy(&end_barrier) != 0) {
    perror("pthread_barrier_destroy(end)");
    return 1;
  }

  if (unlink(path) != 0) {
    perror("unlink");
    return 1;
  }

  printf("race open/close test passed (%d iterations)\n", ITERATIONS);
  return 0;
}
