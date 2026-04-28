#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

static void test_readonly_fd_stays_readonly(void) {
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
}

static void test_read_after_writeonly_open_uses_duplex_shared_handle(void) {
  const char* path = "fd_open_mixed_access_file";
  unlink(path);

  int write_fd = open(path, O_CREAT | O_TRUNC | O_WRONLY, 0644);
  assert(write_fd >= 0);
  assert(write(write_fd, "hello", 5) == 5);

  int read_fd = open(path, O_RDONLY);
  assert(read_fd >= 0);

  __wasi_fdstat_t stat;
  __wasi_errno_t err = __wasi_fd_fdstat_get((__wasi_fd_t)read_fd, &stat);
  assert(err == __WASI_ERRNO_SUCCESS);
  assert((stat.fs_rights_base & (__wasi_rights_t)__WASI_RIGHTS_FD_READ) != 0);
  assert((stat.fs_rights_base & (__wasi_rights_t)__WASI_RIGHTS_FD_WRITE) == 0);

  char buffer[6] = {0};
  assert(read(read_fd, buffer, 5) == 5);
  assert(strcmp(buffer, "hello") == 0);

  assert(close(read_fd) == 0);
  assert(close(write_fd) == 0);
  assert(unlink(path) == 0);
}

int main(void) {
  test_readonly_fd_stays_readonly();
  test_read_after_writeonly_open_uses_duplex_shared_handle();
  printf("All tests passed!\n");
  return 0;
}
