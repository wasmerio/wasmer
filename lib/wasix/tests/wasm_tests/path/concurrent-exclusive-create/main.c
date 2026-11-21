//#ExpectedStdout: ok

#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <unistd.h>

#define ROUNDS 32

static pthread_barrier_t start;

static void* create_files(void* arg) {
  int* created = arg;
  for (int i = 0; i < ROUNDS; i++) {
    char path[32];
    snprintf(path, sizeof(path), "exclusive-%d", i);
    int rc = pthread_barrier_wait(&start);
    assert(rc == 0 || rc == PTHREAD_BARRIER_SERIAL_THREAD);

    int fd = open(path, O_CREAT | O_EXCL | O_WRONLY, 0600);
    created[i] = fd >= 0;
    if (fd >= 0) {
      assert(close(fd) == 0);
    } else {
      assert(errno == EEXIST);
    }
  }
  return NULL;
}

int main(void) {
  pthread_t worker;
  int worker_created[ROUNDS], main_created[ROUNDS];
  assert(pthread_barrier_init(&start, NULL, 2) == 0);
  assert(pthread_create(&worker, NULL, create_files, worker_created) == 0);
  create_files(main_created);
  assert(pthread_join(worker, NULL) == 0);
  assert(pthread_barrier_destroy(&start) == 0);

  for (int i = 0; i < ROUNDS; i++) {
    assert(worker_created[i] + main_created[i] == 1);
  }
  printf("ok");
  return 0;
}
