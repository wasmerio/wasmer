//#MinimalLibc: v2026-07-30.1
//#ExpectedStdout: usr2 deferred until handler returned

#include <assert.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>

// sa_mask names *additional* signals to block while a handler runs, not just
// the one being delivered. A SIGUSR1 handler whose sa_mask contains SIGUSR2
// must not be interrupted by SIGUSR2; that delivery is deferred until the
// handler returns.

static volatile sig_atomic_t usr2_calls = 0;
static volatile sig_atomic_t usr2_seen_inside = -1;

static void usr1_handler(int sig, siginfo_t *info, void *ucontext) {
  (void)sig;
  (void)info;
  (void)ucontext;

  raise(SIGUSR2);
  // Sampled before returning: SIGUSR2 is in our sa_mask, so it must still be
  // pending rather than already handled.
  usr2_seen_inside = usr2_calls;
}

static void usr2_handler(int sig, siginfo_t *info, void *ucontext) {
  (void)sig;
  (void)info;
  (void)ucontext;

  usr2_calls++;
}

int main() {
  struct sigaction sa;

  memset(&sa, 0, sizeof(sa));
  sa.sa_sigaction = usr2_handler;
  sa.sa_flags = SA_SIGINFO;
  sigemptyset(&sa.sa_mask);
  assert(sigaction(SIGUSR2, &sa, NULL) == 0);

  memset(&sa, 0, sizeof(sa));
  sa.sa_sigaction = usr1_handler;
  sa.sa_flags = SA_SIGINFO;
  sigemptyset(&sa.sa_mask);
  sigaddset(&sa.sa_mask, SIGUSR2);
  assert(sigaction(SIGUSR1, &sa, NULL) == 0);

  assert(raise(SIGUSR1) == 0);

  assert(usr2_seen_inside == 0 &&
         "sa_mask should keep SIGUSR2 blocked inside the SIGUSR1 handler");
  assert(usr2_calls == 1 &&
         "SIGUSR2 should be delivered once the SIGUSR1 handler returns");

  printf("usr2 deferred until handler returned\n");
  return 0;
}
