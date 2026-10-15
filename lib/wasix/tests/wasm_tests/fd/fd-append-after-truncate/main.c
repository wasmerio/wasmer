#include <assert.h>
#include <fcntl.h>
#include <string.h>
#include <unistd.h>

int main(void) {
  int fd = open("/tmp/x", O_CREAT | O_RDWR | O_APPEND, 0644);
  assert(write(fd, "12345678901234567", 17) == 17);
  assert(ftruncate(fd, 0) == 0);
  assert(write(fd, "Z", 1) == 1);
  close(fd);

  fd = open("/tmp/x", O_RDONLY);
  char b[8] = {0};
  assert(read(fd, b, sizeof(b)) == 1);
  assert(strcmp(b, "Z") == 0);
}
