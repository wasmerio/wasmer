//#MinimalLibc: v2026-07-30.1
//#ExpectedStdout: blocked, pending, then delivered on unblock

#include <assert.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>

// sigprocmask must actually block: a signal raised while blocked is held
// pending, reported by sigpending(), and delivered when it is unblocked.
// Delivery has to happen before sigprocmask returns, since WASIX only delivers
// signals at syscall boundaries and nothing else would come along to do it.

static volatile sig_atomic_t calls = 0;

static void handler(int sig, siginfo_t *info, void *ucontext) {
  (void)sig;
  (void)info;
  (void)ucontext;

  calls++;
}

int main() {
  struct sigaction sa;
  memset(&sa, 0, sizeof(sa));
  sa.sa_sigaction = handler;
  sa.sa_flags = SA_SIGINFO;
  sigemptyset(&sa.sa_mask);
  assert(sigaction(SIGUSR1, &sa, NULL) == 0);

  sigset_t block;
  sigemptyset(&block);
  sigaddset(&block, SIGUSR1);
  assert(sigprocmask(SIG_BLOCK, &block, NULL) == 0);

  assert(raise(SIGUSR1) == 0);
  assert(calls == 0 && "a blocked signal must not be delivered");

  sigset_t pending;
  sigemptyset(&pending);
  assert(sigpending(&pending) == 0);
  assert(sigismember(&pending, SIGUSR1) == 1 &&
         "sigpending should report the blocked signal");

  assert(sigprocmask(SIG_UNBLOCK, &block, NULL) == 0);
  assert(calls == 1 && "unblocking should deliver the pending signal");

  sigemptyset(&pending);
  assert(sigpending(&pending) == 0);
  assert(sigismember(&pending, SIGUSR1) == 0 &&
         "the signal should no longer be pending once delivered");

  printf("blocked, pending, then delivered on unblock\n");
  return 0;
}
