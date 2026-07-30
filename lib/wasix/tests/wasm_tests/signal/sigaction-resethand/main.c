//#MinimalLibc: v2026-07-30.1
//#ExpectedStdout: resethand ok

#include <assert.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>

// POSIX resets a handler installed with SA_RESETHAND to SIG_DFL on entry, so
// re-raising the same signal from inside the handler takes the default action
// instead of re-entering the handler. Node's `SignalExit` relies on exactly
// that: it re-raises to let the default disposition terminate the process.
// Without the reset it recurses until the stack is exhausted.
//
// The reset is asserted directly rather than by re-raising, because unbounded
// re-entry exhausts the host stack and would take the test process down with
// it instead of failing this one test.

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
  sa.sa_flags = SA_SIGINFO | SA_RESETHAND;
  sigfillset(&sa.sa_mask);
  assert(sigaction(SIGUSR1, &sa, NULL) == 0);

  assert(raise(SIGUSR1) == 0);
  assert(calls == 1 && "handler should have run once");

  struct sigaction old;
  memset(&old, 0, sizeof(old));
  assert(sigaction(SIGUSR1, NULL, &old) == 0);
  assert(old.sa_handler == SIG_DFL &&
         "SA_RESETHAND should reset the disposition to SIG_DFL");

  printf("resethand ok\n");
  return 0;
}
