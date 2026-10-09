#include <assert.h>
#include <signal.h>
#include <stdint.h>
#include <wasi/api.h>
#include <wasi/api_wasix.h>

static volatile sig_atomic_t calls = 0;

static void handler(int sig) { calls++; }

int main(void) {
  __wasi_pid_t pid;
  assert(__wasi_proc_id(&pid) == __WASI_ERRNO_SUCCESS);
  assert(__wasi_proc_signal(pid, __WASI_SIGNAL_NONE) == __WASI_ERRNO_SUCCESS);

  assert(signal(SIGUSR1, handler) != SIG_ERR);
  assert(__wasi_proc_signal(pid, __WASI_SIGNAL_USR1) == __WASI_ERRNO_SUCCESS);
  assert(calls == 1);

  assert(__wasi_proc_signal(INT32_MAX, __WASI_SIGNAL_TERM) ==
         __WASI_ERRNO_SRCH);
  assert(__wasi_proc_signal(INT32_MAX, __WASI_SIGNAL_NONE) ==
         __WASI_ERRNO_SRCH);
  // A negative PID cannot find a process either: groups are not implemented.
  assert(__wasi_proc_signal(-pid, __WASI_SIGNAL_KILL) == __WASI_ERRNO_SRCH);
}
