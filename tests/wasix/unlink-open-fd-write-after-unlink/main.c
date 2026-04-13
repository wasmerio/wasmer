#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int main() {
  int fd = open("/tmp/test.txt", O_CREAT | O_WRONLY | O_TRUNC, 0644);
  if (fd == -1) {
    perror("open");
    return 1;
  }
  printf("open succeeded\n");

  if (unlink("/tmp/test.txt") == -1) {
    perror("unlink");
    return 1;
  }
  printf("unlink succeeded\n");

  FILE *fp = fdopen(fd, "wr");
  if (fp == NULL) {
    perror("fdopen");
    return 1;
  }
  printf("fdopen succeeded\n");

  char memory_buffer[1025];
  memset(memory_buffer, 'a', sizeof(memory_buffer));
  size_t n = fwrite(memory_buffer, 1, sizeof(memory_buffer), fp);
  if (n != sizeof(memory_buffer)) {
    fprintf(stderr, "short write: wrote %zu bytes\n", n);
    return 1;
  }
  if (ferror(fp)) {
    perror("fwrite");
    return 1;
  }
  printf("writing succeeded\n");

  if (fclose(fp) != 0) {
    perror("fclose");
    return 1;
  }

  return 0;
}
