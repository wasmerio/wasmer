/* Linux: opening /dev/fd/N (or /proc/self/fd/N) of an anonymous pipe whose
 * writers are all gone must not block (fs/pipe.c fifo_open skips
 * wait_for_partner for pipefs inodes); the reader sees buffered data, then EOF. */
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>
int main(void) {
  int p[2]; char path[32], buf[16];
  if (pipe(p)) return perror("pipe"), 1;
  write(p[1], "ps\n", 3);
  close(p[1]);
  snprintf(path, sizeof path, "/dev/fd/%d", p[0]);
  int fd = open(path, O_RDONLY);
  if (fd < 0) return perror("open"), 1;
  ssize_t n = read(fd, buf, sizeof buf);
  printf("read %zd: %.*s", n, (int)(n > 0 ? n : 0), buf);
  printf("eof read: %zd\n", read(fd, buf, sizeof buf));
  return 0;
}
