#include <assert.h>
#include <fcntl.h>
#include <unistd.h>

int main(void) {
  int fd = open("/tmp/sparse", O_CREAT | O_RDWR, 0644);
  write(fd, "12345678901234567", 17);
  ftruncate(fd, 0);
  write(fd, "Z", 1);
  lseek(fd, 0, SEEK_SET);

  char b[32] = {1};
  assert(read(fd, b, sizeof(b)) == 18);
  for (int i = 0; i < 17; i++) {
    assert(b[i] == 0);
  }
  assert(b[17] == 'Z');
}
