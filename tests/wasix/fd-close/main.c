#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <unistd.h>

int main() {
  int fd = socket(AF_INET, SOCK_STREAM, 0);
  if (fd < 0) {
    perror("socket");
    return 1;
  }

  if (close(fd) < 0) {
    perror("socket close");
    return 1;
  }

  errno = 0;
  if (close(fd) == 0) {
    fprintf(stderr, "expected second socket close to fail\n");
    return 1;
  }
  if (errno != EBADF) {
    fprintf(stderr, "expected EBADF from second socket close, got %d\n", errno);
    return 1;
  }

  fd = open("/bin", O_RDONLY | O_DIRECTORY);
  if (fd < 0) {
    perror("open dir");
    return 1;
  }

  if (close(fd) < 0) {
    perror("dir close");
    return 1;
  }

  return 0;
}
