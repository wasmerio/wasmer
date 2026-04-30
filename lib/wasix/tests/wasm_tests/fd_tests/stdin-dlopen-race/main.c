#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cond = PTHREAD_COND_INITIALIZER;
static int reader_ready = 0;

typedef int (*side_value_t)(void);

static void* reader_thread(void* arg) {
  (void)arg;

  /* Signal to the main thread that we are about to enter fd_read. */
  pthread_mutex_lock(&mutex);
  reader_ready = 1;
  pthread_cond_signal(&cond);
  pthread_mutex_unlock(&mutex);

  /* Block in fd_read on stdin.  The host provides a pipe whose write end is
   * never written to, so this call parks indefinitely.  The process will be
   * terminated by main() returning before this ever unblocks. */
  char buf[64];
  read(STDIN_FILENO, buf, sizeof(buf));

  return NULL;
}

int main(void) {
  pthread_t t;
  pthread_create(&t, NULL, reader_thread, NULL);

  /* Wait until the reader thread has set reader_ready (it is about to call
   * read(), if it hasn't already). */
  pthread_mutex_lock(&mutex);
  while (!reader_ready) pthread_cond_wait(&cond, &mutex);
  pthread_mutex_unlock(&mutex);

  printf("reader_ready\n");

  /* Give the reader thread time to actually enter the blocking read before
   * attempting dlopen, so this test exercises the intended race reliably. */
  sleep(1);

  /* Load a shared library while the other thread is blocked in fd_read.
   * Before the fix this would deadlock because fd_read held a lock that
   * the DL subsystem also needed. */
  void* handle = dlopen("libside.so", RTLD_NOW | RTLD_LOCAL);
  if (!handle) {
    fprintf(stderr, "dlopen failed: %s\n", dlerror());
    return 1;
  }

  printf("dlopen_succeeded_after_reader_ready\n");

  side_value_t side_value = (side_value_t)dlsym(handle, "side_value");
  if (!side_value) {
    fprintf(stderr, "dlsym failed: %s\n", dlerror());
    dlclose(handle);
    return 1;
  }

  printf("side_value=%d\n", side_value());
  printf("sequence_ok\n");
  dlclose(handle);

  fflush(stdout);
  _Exit(0);
}
