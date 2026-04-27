#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cond = PTHREAD_COND_INITIALIZER;
static int reader_ready = 0;
static volatile sig_atomic_t handler_called = 0;
static int read_errno = 0;

static void signal_handler(int sig) {
  (void)sig;
  handler_called = 1;
}

static void* reader_thread(void* arg) {
  (void)arg;

  pthread_mutex_lock(&mutex);
  reader_ready = 1;
  pthread_cond_signal(&cond);
  pthread_mutex_unlock(&mutex);

  char buf[64];
  ssize_t ret = read(STDIN_FILENO, buf, sizeof(buf));
  if (ret >= 0) {
    fprintf(stderr, "read unexpectedly succeeded: %zd\n", ret);
    return (void*)1;
  }

  read_errno = errno;
  return NULL;
}

int main(void) {
  struct sigaction sa;
  memset(&sa, 0, sizeof(sa));
  sa.sa_handler = signal_handler;
  sigemptyset(&sa.sa_mask);
  sa.sa_flags = 0;
  if (sigaction(SIGUSR1, &sa, NULL) != 0) {
    perror("sigaction");
    return 1;
  }

  pthread_t t;
  if (pthread_create(&t, NULL, reader_thread, NULL) != 0) {
    perror("pthread_create");
    return 1;
  }

  pthread_mutex_lock(&mutex);
  while (!reader_ready) pthread_cond_wait(&cond, &mutex);
  pthread_mutex_unlock(&mutex);

  printf("reader_ready\n");

  // Give the reader thread time to actually enter the blocking read.
  sleep(1);

  if (pthread_kill(t, SIGUSR1) != 0) {
    perror("pthread_kill");
    return 1;
  }

  printf("signal_sent\n");

  void* thread_ret = NULL;
  if (pthread_join(t, &thread_ret) != 0) {
    perror("pthread_join");
    return 1;
  }
  if (thread_ret != NULL) {
    return 1;
  }

  if (!handler_called) {
    fprintf(stderr, "signal handler was not called\n");
    return 1;
  }
  printf("handler_called\n");

  if (read_errno != EINTR) {
    fprintf(stderr, "expected EINTR, got %d\n", read_errno);
    return 1;
  }

  printf("read_errno=EINTR\n");
  printf("sequence_ok\n");
  return 0;
}