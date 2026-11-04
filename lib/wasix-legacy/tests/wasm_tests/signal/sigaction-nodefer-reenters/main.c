//#MinimalLibc: v2026-07-30.1
//#ExpectedStdout: calls=5 max_depth=5

#include <assert.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>

// SA_NODEFER opts out of adding the delivered signal to the mask for the
// duration of its handler, so a signal re-raised from inside the handler
// re-enters it immediately instead of being deferred.
//
// sa_mask must be empty here: it is applied on top of SA_NODEFER, so a
// sigfillset() mask would block SIGUSR1 anyway and defeat the opt-out.
//
// The re-raise is gated on `calls < LIMIT` in guest code, bounding the nesting
// to 5 frames. Paired with sigaction-mask-defers, the same program without
// SA_NODEFER, which expects max_depth == 1.

#define LIMIT 5

static volatile sig_atomic_t calls = 0;
static volatile sig_atomic_t depth = 0;
static volatile sig_atomic_t max_depth = 0;

static void handler(int sig, siginfo_t* info, void* ucontext) {
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
  sa.sa_flags = SA_SIGINFO | SA_NODEFER;
  sigemptyset(&sa.sa_mask);
  assert(sigaction(SIGUSR1, &sa, NULL) == 0);

  assert(raise(SIGUSR1) == 0);

  assert(calls == LIMIT && "handler should run once per raise");
  assert(max_depth == LIMIT && "SA_NODEFER should allow the handler to nest");
  assert(depth == 0 && "every invocation should have returned");

  printf("calls=%d max_depth=%d\n", (int)calls, (int)max_depth);
  return 0;
}
