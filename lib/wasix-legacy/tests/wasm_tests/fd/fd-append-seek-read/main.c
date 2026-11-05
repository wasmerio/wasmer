#include <assert.h>
#include <fcntl.h>
#include <string.h>
#include <unistd.h>

int main(void) {
  int fd = open("/tmp/append_seek", O_CREAT | O_RDWR | O_APPEND, 0644);
  write(fd, "abc", 3);
  lseek(fd, 0, SEEK_SET);

  char b[4] = {0};
  assert(read(fd, b, sizeof(b)) == 3);
  assert(strcmp(b, "abc") == 0);
}
