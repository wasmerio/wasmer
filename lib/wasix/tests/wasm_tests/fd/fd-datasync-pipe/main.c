#include <assert.h>
#include <errno.h>
#include <unistd.h>

int main(void) {
  int fds[2];
  assert(pipe(fds) == 0);

  // Flushing pipes should fail with EINVAL as it does on Linux
  // Regression test for this previously returning EIO
  errno = 0;
  assert(fdatasync(fds[0]) == -1);
  assert(errno == EINVAL);

  errno = 0;
  assert(fdatasync(fds[1]) == -1);
  assert(errno == EINVAL);

  // Similarly, this should also fail with EINVAL
  // Regression test for this previously returning success
  // from special handling of stdin
  assert(dup2(fds[0], STDIN_FILENO) == STDIN_FILENO);
  errno = 0;
  assert(fdatasync(STDIN_FILENO) == -1);
  assert(errno == EINVAL);

  close(fds[0]);
  close(fds[1]);
  close(STDIN_FILENO);
  return 0;
}
