#include <assert.h>
#include <fcntl.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

int main(void) {
  int fd = open("/tmp/proc-spawn2-dup2-huge-fd", O_CREAT | O_RDWR, 0644);
  assert(fd >= 0);

  __wasi_proc_spawn_fd_op_t op = {0};
  op.cmd = __WASI_PROC_SPAWN_FD_OP_NAME_DUP2;
  op.fd = 65536;
  op.src_fd = (__wasi_fd_t)fd;

  __wasi_pid_t pid = 0;
  __wasi_errno_t err =
      __wasi_proc_spawn2("missing-process", "", "", &op, 1, 0, 0, 0, "", &pid);
  assert(err == __WASI_ERRNO_BADF);

  assert(close(fd) == 0);
}
