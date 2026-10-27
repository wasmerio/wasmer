/* Linux read_zero() fills the whole buffer: read(/dev/zero, 1 MiB) returns 1048576.
 * dd bs=1M count=N relies on it (LTP du01.sh, many others). */
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
int main(void) {
  size_t n = 1 << 20; char *b = malloc(n);
  int fd = open("/dev/zero", O_RDONLY);
  printf("read(/dev/zero, %zu) = %zd\n", n, read(fd, b, n));
  fd = open("/dev/urandom", O_RDONLY);
  printf("read(/dev/urandom, %zu) = %zd\n", n, read(fd, b, n));
  return 0;
}
