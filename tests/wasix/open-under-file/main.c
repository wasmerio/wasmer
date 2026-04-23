#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

int main(void) {
  int fd = open("parent", O_CREAT | O_WRONLY | O_TRUNC, 0644);
  if (fd < 0) {
    perror("open parent");
    return 1;
  }
  close(fd);

  fd = open("parent/child", O_CREAT | O_EXCL | O_RDWR, 0600);
  if (fd != -1) {
    fprintf(stderr, "open unexpectedly succeeded\n");
    close(fd);
    return 1;
  }

  if (errno != ENOTDIR) {
    fprintf(stderr, "expected ENOTDIR, got %d\n", errno);
    return 1;
  }

  printf("0");
  return 0;
}
