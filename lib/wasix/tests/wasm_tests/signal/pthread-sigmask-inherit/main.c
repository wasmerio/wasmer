//#MinimalLibc: v2026-07-30.1
//#ExpectedStdout: child inherited the blocked mask

#include <assert.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>

// A new thread inherits its creator's signal mask, and the mask is per-thread:
// unblocking in the child must not affect the parent.

static volatile int child_saw_blocked = 0;
static volatile int child_query_ok = 0;

static void* child(void* arg) {
  (void)arg;

  sigset_t mask;
  sigemptyset(&mask);
  child_query_ok = (pthread_sigmask(SIG_BLOCK, NULL, &mask) == 0);
  child_saw_blocked = sigismember(&mask, SIGUSR1);

  sigset_t unblock;
  sigemptyset(&unblock);
  sigaddset(&unblock, SIGUSR1);
  pthread_sigmask(SIG_UNBLOCK, &unblock, NULL);

  return NULL;
}

int main() {
  sigset_t block;
  sigemptyset(&block);
  sigaddset(&block, SIGUSR1);
  assert(pthread_sigmask(SIG_BLOCK, &block, NULL) == 0);

  pthread_t thread;
  assert(pthread_create(&thread, NULL, child, NULL) == 0);
  assert(pthread_join(thread, NULL) == 0);

  assert(child_query_ok && "pthread_sigmask should succeed in the child");
  assert(child_saw_blocked == 1 &&
         "child should inherit the creator's blocked mask");

  // The child unblocked SIGUSR1 for itself only; ours must still be blocked.
  sigset_t mine;
  sigemptyset(&mine);
  assert(pthread_sigmask(SIG_BLOCK, NULL, &mine) == 0);
  assert(sigismember(&mine, SIGUSR1) == 1 &&
         "the mask should be per-thread, not shared");

  printf("child inherited the blocked mask\n");
  return 0;
}
