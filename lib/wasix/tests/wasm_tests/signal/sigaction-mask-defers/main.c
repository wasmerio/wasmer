//#MinimalLibc: v2026-07-30.1
//#ExpectedStdout: calls=5 max_depth=1

#include <assert.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>

// POSIX blocks the delivered signal for the duration of its handler (sa_mask
// plus the signal itself, the latter unless SA_NODEFER). So a signal re-raised
// from inside the handler is deferred and delivered once the handler returns,
// running the handler again sequentially rather than nesting it.
//
// The re-raise is gated on `calls < LIMIT` in guest code, so the recursion is
// bounded no matter how libc behaves: even with masking entirely broken this
// nests 5 deep rather than exhausting the stack.
//
// Paired with sigaction-nodefer-reenters, which is the same program with
// SA_NODEFER and therefore expects max_depth == LIMIT.

#define LIMIT 5

static volatile sig_atomic_t calls = 0;
static volatile sig_atomic_t depth = 0;
static volatile sig_atomic_t max_depth = 0;

static void handler(int sig, siginfo_t *info, void *ucontext) {
  (void)info;
  (void)ucontext;

  calls++;
  depth++;
  if (depth > max_depth) {
    max_depth = depth;
  }
  if (calls < LIMIT) {
    raise(sig);
  }
  depth--;
}

int main() {
  struct sigaction sa;
  memset(&sa, 0, sizeof(sa));
  sa.sa_sigaction = handler;
  sa.sa_flags = SA_SIGINFO;
  sigfillset(&sa.sa_mask);
  assert(sigaction(SIGUSR1, &sa, NULL) == 0);

  // Every deferred delivery is drained before the raise that triggered the
  // first one returns, so no further syscall is needed to observe them.
  assert(raise(SIGUSR1) == 0);

  assert(calls == LIMIT && "handler should run once per raise");
  assert(max_depth == 1 && "a blocked signal must not re-enter its handler");
  assert(depth == 0 && "every invocation should have returned");

  printf("calls=%d max_depth=%d\n", (int)calls, (int)max_depth);
  return 0;
}
