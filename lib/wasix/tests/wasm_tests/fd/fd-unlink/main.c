#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void) {
  char buf[32];
  struct stat st;
  memset(buf, 'A', sizeof(buf));

  int fd = open("/tmp/test.txt", O_CREAT | O_RDWR | O_TRUNC, 0644);
  if (fd == -1) {
    perror("open first");
    return 1;
  }

  assert(fd >= 0);
  assert(write(fd, buf, sizeof(buf)) == sizeof(buf));
  assert(unlink("/tmp/test.txt") == 0);

  errno = 0;
  assert(stat("/tmp/test.txt", &st) == -1);
  assert(errno == ENOENT);

  fd = open("/tmp/test.txt", O_RDWR);
  assert(fd == -1);
}
