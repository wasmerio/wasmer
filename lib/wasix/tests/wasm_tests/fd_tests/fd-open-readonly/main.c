#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

int main(void) {
  const char* path = "fd_open_readonly_file";
  unlink(path);

  int fd = open(path, O_CREAT | O_TRUNC | O_RDWR, 0644);
  assert(fd >= 0);
  assert(write(fd, "hello", 5) == 5);
  assert(close(fd) == 0);

  fd = open(path, O_RDONLY);
  assert(fd >= 0);

  __wasi_fdstat_t stat;
  __wasi_errno_t err = __wasi_fd_fdstat_get((__wasi_fd_t)fd, &stat);
  assert(err == __WASI_ERRNO_SUCCESS);
  assert((stat.fs_rights_base & (__wasi_rights_t)__WASI_RIGHTS_FD_READ) != 0);
  assert((stat.fs_rights_base & (__wasi_rights_t)__WASI_RIGHTS_FD_WRITE) == 0);

  errno = 0;
  assert(write(fd, "!", 1) == -1);
  assert(errno != 0);

  assert(close(fd) == 0);

  char buffer[6] = {0};
  fd = open(path, O_RDONLY);
  assert(fd >= 0);
  assert(read(fd, buffer, 5) == 5);
  assert(strcmp(buffer, "hello") == 0);
  assert(close(fd) == 0);
  assert(unlink(path) == 0);

  printf("All tests passed!\n");
  return 0;
}
