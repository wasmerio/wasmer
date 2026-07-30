//#MinimalLibc: v2026-07-30.1
//#ExpectedStdout: plain handler ok
//#ExpectedStdout: siginfo handler ok

#include <assert.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>

// A handler installed through `sa_sigaction` with `SA_SIGINFO` takes three
// arguments, while one installed through `sa_handler` takes a single one. On
// wasm these are distinct function types, so dispatching a three-argument
// handler through a one-argument `call_indirect` traps with "indirect call
// type mismatch" and kills the instance. Cover both dispatch forms.

static volatile sig_atomic_t plain_calls = 0;
static volatile sig_atomic_t info_calls = 0;
static volatile sig_atomic_t info_signo = 0;
static volatile sig_atomic_t info_arg_ok = 0;

static void plain_handler(int sig) {
  (void)sig;
  plain_calls++;
}

static void info_handler(int sig, siginfo_t *info, void *ucontext) {
  (void)ucontext;
  info_calls++;
  info_signo = sig;
  info_arg_ok = (info != NULL && info->si_signo == sig);
}

int main() {
  struct sigaction sa;

  memset(&sa, 0, sizeof(sa));
  sa.sa_handler = plain_handler;
  sigfillset(&sa.sa_mask);
  assert(sigaction(SIGUSR1, &sa, NULL) == 0);

  assert(raise(SIGUSR1) == 0);
  assert(plain_calls == 1 && "one-argument handler should have run");
  printf("plain handler ok\n");

  memset(&sa, 0, sizeof(sa));
  sa.sa_sigaction = info_handler;
  sa.sa_flags = SA_SIGINFO;
  sigfillset(&sa.sa_mask);
  assert(sigaction(SIGUSR2, &sa, NULL) == 0);

  assert(raise(SIGUSR2) == 0);
  assert(info_calls == 1 && "three-argument handler should have run");
  assert(info_signo == SIGUSR2 && "handler should receive the signal number");
  assert(info_arg_ok && "handler should receive a populated siginfo_t");
  printf("siginfo handler ok\n");

  return 0;
}
