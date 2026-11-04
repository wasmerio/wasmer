#include <assert.h>
#include <errno.h>
#include <poll.h>
#include <unistd.h>

// Regression test for a bug where special handling of stdio was causing
// redirected stdio to error out on basic operations
static void assert_pollout_ready_after_dup2(int stdio_fd) {
  int fds[2];
  assert(pipe(fds) == 0);

  assert(dup2(fds[1], stdio_fd) == stdio_fd);
  close(fds[1]);

  struct pollfd pfd = {.fd = stdio_fd, .events = POLLOUT, .revents = 0};
  errno = 0;
  int ready = poll(&pfd, 1, 10);
  assert(ready == 1);
  assert(pfd.revents & POLLOUT);

  close(fds[0]);
  close(stdio_fd);
}

int main(void) {
  assert_pollout_ready_after_dup2(STDOUT_FILENO);
  assert_pollout_ready_after_dup2(STDERR_FILENO);
  return 0;
}
