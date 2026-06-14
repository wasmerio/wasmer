#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <unistd.h>

int main(void) {
  int fd = open("/tmp/renumber-negative-target", O_CREAT | O_RDWR, 0644);
  assert(fd >= 0);

  errno = 0;
  assert(dup2(fd, -1) == -1);
  assert(errno == EBADF);

  errno = 0;
  assert(dup2(-1, -1) == -1);
  assert(errno == EBADF);

  assert(close(fd) == 0);
}
