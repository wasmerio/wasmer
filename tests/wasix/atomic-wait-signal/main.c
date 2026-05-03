#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

static _Atomic uint32_t wait_word = 0;

int main(void) {
  pid_t parent = getpid();
  pid_t child = fork();

  if (child < 0) {
    perror("fork");
    return EXIT_FAILURE;
  }

  if (child == 0) {
    usleep(100 * 1000);
    if (kill(parent, SIGKILL) != 0) {
      perror("kill");
      _Exit(EXIT_FAILURE);
    }
    _Exit(EXIT_SUCCESS);
  }

  puts("waiting");
  fflush(stdout);

  __builtin_wasm_memory_atomic_wait32((int *)&wait_word, 0, -1LL);

  puts("woken");
  fflush(stdout);

  // The atomic wait wakeup makes the main thread runnable again. This syscall
  // gives WASIX a chance to process the pending terminating signal.
  sleep(1);

  return EXIT_FAILURE;
}
