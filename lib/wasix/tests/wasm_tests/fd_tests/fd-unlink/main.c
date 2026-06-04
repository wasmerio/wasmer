#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void) {
  char buf[32];
  struct stat st;
  memset(buf, 'A', sizeof(buf));

  int fd = open("fixture", O_RDWR);
  assert(fd >= 0);
  assert(write(fd, buf, sizeof(buf)) == sizeof(buf));
  assert(unlink("fixture") == 0);

  errno = 0;
  assert(stat("fixture", &st) == -1);
  assert(errno == ENOENT);

  fd = open("fixture", O_RDWR);
  assert(fd == -1);
}
