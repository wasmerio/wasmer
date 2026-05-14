//#Tempdir: true
#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(void) {
  int fd = open("/tmp/same-path", O_CREAT | O_RDWR, 0644);
  assert(write(fd, "ok", 2) == 2);
  assert(close(fd) == 0);

  assert(rename("/tmp/same-path", "/tmp/same-path") == 0);

  char buf[3] = {0};
  fd = open("/tmp/same-path", O_RDONLY);
  assert(fd >= 0);
  assert(read(fd, buf, sizeof(buf)) == 2);
  assert(strcmp(buf, "ok") == 0);
}
