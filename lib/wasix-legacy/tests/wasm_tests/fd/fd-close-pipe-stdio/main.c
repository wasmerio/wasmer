#include <assert.h>
#include <errno.h>
#include <unistd.h>

// close should always succeed on a valid fd
// Guards against a regression where previously EINVAL would be incorrectly
// propagated from an internal call to flush
static void assert_close_ok_after_dup2(int stdio_fd) {
  int fds[2];
  assert(pipe(fds) == 0);

  assert(dup2(fds[1], stdio_fd) == stdio_fd);
  close(fds[1]);

  errno = 0;
  int ret = close(stdio_fd);
  assert(ret == 0);

  close(fds[0]);
}

int main(void) {
  assert_close_ok_after_dup2(STDOUT_FILENO);
  assert_close_ok_after_dup2(STDERR_FILENO);
  return 0;
}
