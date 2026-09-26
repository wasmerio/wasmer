//#BuildEnv: WASIXCC_WASM_EXCEPTIONS=no
//#ExpectedStdout: nonblocking wait passed
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <sys/wait.h>
#include <unistd.h>

int main(void) {
  int fds[2];
  assert(pipe(fds) == 0);
  pid_t child = fork();
  assert(child >= 0);
  if (child == 0) {
    close(fds[1]);
    char byte;
    assert(read(fds[0], &byte, 1) == 1);
    _exit(42);
  }

  close(fds[0]);
  int status;
  // The child cannot exit until this nonblocking wait returns.
  assert(waitpid(-1, &status, WNOHANG) == 0);
  assert(waitpid(-1, &status, WNOHANG) == 0);
  assert(write(fds[1], "x", 1) == 1);
  close(fds[1]);
  pid_t result;
  do {
    result = waitpid(-1, &status, WNOHANG);
    if (result == 0) usleep(1000);
  } while (result == 0);
  assert(result == child);
  assert(WIFEXITED(status) && WEXITSTATUS(status) == 42);
  assert(waitpid(-1, &status, WNOHANG) == -1);
  assert(errno == ECHILD);

  child = fork();
  assert(child >= 0);
  if (child == 0) _exit(43);
  assert(waitpid(-1, &status, 0) == child);
  assert(WIFEXITED(status) && WEXITSTATUS(status) == 43);
  puts("nonblocking wait passed");
  errno = 0;
  assert(waitpid(123456, &status, WNOHANG) == -1);
  assert(errno == ECHILD);

  return 0;
}
