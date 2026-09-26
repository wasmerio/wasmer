//#ExpectedStdout: redirected stdio passed
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>
#include <wasi/api.h>

int main(void) {
  int saved[3];
  for (int fd = 0; fd < 3; fd++) {
    saved[fd] = dup(fd);
    assert(saved[fd] >= 0);
  }
  int file = open("/tmp/redirected-stdio", O_CREAT | O_RDWR | O_TRUNC, 0600);
  assert(file >= 0);
  for (int fd = 0; fd < 3; fd++) {
    assert(dup2(file, fd) == fd);
    __wasi_fdstat_t stat;
    assert(__wasi_fd_fdstat_get(fd, &stat) == 0);
    assert(stat.fs_filetype == __WASI_FILETYPE_REGULAR_FILE);
    assert(!isatty(fd));
  }
  __wasi_tty_t tty;
  int result = __wasi_tty_get(&tty);
  if (result == 0) {
    assert(!tty.stdin_tty && !tty.stdout_tty && !tty.stderr_tty);
  }
  for (int fd = 0; fd < 3; fd++) {
    assert(dup2(saved[fd], fd) == fd);
    close(saved[fd]);
  }
  close(file);
  puts("redirected stdio passed");
  return 0;
}
