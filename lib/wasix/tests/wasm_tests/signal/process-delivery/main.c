#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api.h>

static _Thread_local int is_worker;
static atomic_int main_hits;
static atomic_int worker_hits;
static atomic_int ready;
static atomic_int stop;

static void handler(int signal) {
  assert(signal == SIGUSR1 || signal == SIGPIPE);
  atomic_fetch_add(is_worker ? &worker_hits : &main_hits, 1);
}

static void reset_counts(void) {
  atomic_store(&main_hits, 0);
  atomic_store(&worker_hits, 0);
  atomic_store(&ready, 0);
  atomic_store(&stop, 0);
}

static void install_handler(int signal) {
  struct sigaction action = {0};
  action.sa_handler = handler;
  sigemptyset(&action.sa_mask);
  assert(sigaction(signal, &action, NULL) == 0);
}

static void wait_until(atomic_int* value, int expected) {
  for (int i = 0; i < 5000 && atomic_load(value) != expected; ++i) {
    usleep(1000);
  }
  assert(atomic_load(value) == expected);
}

static void* syscall_worker(void* argument) {
  (void)argument;
  is_worker = 1;
  atomic_fetch_add(&ready, 1);
  while (!atomic_load(&stop)) {
    (void)getpid();
    sched_yield();
  }
  return NULL;
}

static int process_one_recipient(void) {
  pthread_t workers[2];
  reset_counts();
  install_handler(SIGUSR1);
  for (int i = 0; i < 2; ++i) {
    assert(pthread_create(&workers[i], NULL, syscall_worker, NULL) == 0);
  }
  wait_until(&ready, 2);

  assert(kill(getpid(), SIGUSR1) == 0);
  usleep(50000);
  atomic_store(&stop, 1);
  for (int i = 0; i < 2; ++i) {
    assert(pthread_join(workers[i], NULL) == 0);
  }

  assert(atomic_load(&main_hits) == 1);
  assert(atomic_load(&worker_hits) == 0);
  puts("process signal reached one recipient");
  return EXIT_SUCCESS;
}

static void* targeted_worker(void* argument) {
  (void)argument;
  is_worker = 1;
  atomic_store(&ready, 1);
  for (int i = 0; i < 100000 && atomic_load(&worker_hits) == 0; ++i) {
    (void)getpid();
    sched_yield();
  }
  return NULL;
}

static int target_worker(void) {
  pthread_t worker;
  reset_counts();
  install_handler(SIGUSR1);
  assert(pthread_create(&worker, NULL, targeted_worker, NULL) == 0);
  wait_until(&ready, 1);

  assert(pthread_kill(worker, SIGUSR1) == 0);
  assert(pthread_join(worker, NULL) == 0);
  (void)getpid();

  assert(atomic_load(&main_hits) == 0);
  assert(atomic_load(&worker_hits) == 1);
  puts("thread signal reached its target");
  return EXIT_SUCCESS;
}

static void* registering_worker(void* argument) {
  (void)argument;
  is_worker = 1;
  install_handler(SIGUSR1);
  atomic_store(&ready, 1);
  while (!atomic_load(&stop)) {
    (void)getpid();
    sched_yield();
  }
  return NULL;
}

static int worker_registration(void) {
  pthread_t worker;
  reset_counts();
  assert(pthread_create(&worker, NULL, registering_worker, NULL) == 0);
  wait_until(&ready, 1);

  assert(kill(getpid(), SIGUSR1) == 0);
  atomic_store(&stop, 1);
  assert(pthread_join(worker, NULL) == 0);

  assert(atomic_load(&main_hits) == 1);
  assert(atomic_load(&worker_hits) == 0);
  puts("worker registration applied process-wide");
  return EXIT_SUCCESS;
}

static void* raising_worker(void* argument) {
  (void)argument;
  is_worker = 1;
  assert(__wasi_proc_raise((__wasi_signal_t)SIGUSR1) == __WASI_ERRNO_SUCCESS);
  return NULL;
}

static int raise_on_worker(void) {
  pthread_t worker;
  reset_counts();
  install_handler(SIGUSR1);
  assert(pthread_create(&worker, NULL, raising_worker, NULL) == 0);
  assert(pthread_join(worker, NULL) == 0);
  (void)getpid();

  assert(atomic_load(&main_hits) == 0);
  assert(atomic_load(&worker_hits) == 1);
  puts("proc_raise stayed on its calling thread");
  return EXIT_SUCCESS;
}

static void* pipe_worker(void* argument) {
  int fd = *(int*)argument;
  is_worker = 1;
  errno = 0;
  assert(write(fd, "x", 1) == -1);
  assert(errno == EPIPE);
  return NULL;
}

static int sigpipe_on_writer(void) {
  int fds[2];
  pthread_t worker;
  reset_counts();
  install_handler(SIGPIPE);
  assert(pipe(fds) == 0);
  assert(close(fds[0]) == 0);
  assert(pthread_create(&worker, NULL, pipe_worker, &fds[1]) == 0);
  assert(pthread_join(worker, NULL) == 0);
  (void)getpid();
  assert(close(fds[1]) == 0);

  assert(atomic_load(&main_hits) == 0);
  assert(atomic_load(&worker_hits) == 1);
  puts("SIGPIPE reached the writing thread");
  return EXIT_SUCCESS;
}

int main(int argc, char** argv) {
  assert(argc == 2);
  if (strcmp(argv[1], "process") == 0) return process_one_recipient();
  if (strcmp(argv[1], "thread") == 0) return target_worker();
  if (strcmp(argv[1], "registration") == 0) return worker_registration();
  if (strcmp(argv[1], "raise") == 0) return raise_on_worker();
  if (strcmp(argv[1], "sigpipe") == 0) return sigpipe_on_writer();
  return EXIT_FAILURE;
}
