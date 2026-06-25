#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>

static void fail(const char* msg) {
  perror(msg);
  exit(1);
}

int main(void) {
  struct stat st;

  if (stat("/dev/null", &st) != 0) {
    fail("stat /dev/null");
  }
  if (!S_ISCHR(st.st_mode)) {
    fprintf(stderr, "stat(/dev/null) mode %#o is not char device\n",
            st.st_mode);
    return 1;
  }

  int fd = open("/dev/null", O_RDWR);
  if (fd < 0) {
    fail("open /dev/null");
  }
  if (fstat(fd, &st) != 0) {
    fail("fstat /dev/null");
  }
  if (!S_ISCHR(st.st_mode)) {
    fprintf(stderr, "fstat(/dev/null) mode %#o is not char device\n",
            st.st_mode);
    close(fd);
    return 1;
  }

  close(fd);
  printf("0");
  return 0;
}
