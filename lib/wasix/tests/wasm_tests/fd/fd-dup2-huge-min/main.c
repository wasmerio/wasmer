#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

int main(void) {
  int fd = open("/tmp/dup2-huge-min", O_CREAT | O_RDWR, 0644);
  assert(fd >= 0);

  __wasi_fd_t ret = 0;
  __wasi_errno_t err = __wasi_fd_dup2((__wasi_fd_t)fd, 65536, 0, &ret);
  assert(err == __WASI_ERRNO_INVAL);

  assert(close(fd) == 0);
}
