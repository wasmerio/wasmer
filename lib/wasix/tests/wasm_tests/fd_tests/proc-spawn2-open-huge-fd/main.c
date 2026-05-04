#include <assert.h>
#include <fcntl.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

int main(void) {
  const char* path = "/tmp/proc-spawn2-open-huge-fd";
  int fd = open(path, O_CREAT | O_RDWR, 0644);
  assert(fd >= 0);
  assert(close(fd) == 0);

  __wasi_proc_spawn_fd_op_t op = {0};
  op.cmd = __WASI_PROC_SPAWN_FD_OP_NAME_OPEN;
  op.fd = 65536;
  op.path = (uint8_t*)path;
  op.path_len = strlen(path);
  op.fs_rights_base = __WASI_RIGHTS_FD_READ;
  op.fs_rights_inheriting = __WASI_RIGHTS_FD_READ;

  __wasi_pid_t pid = 0;
  __wasi_errno_t err =
      __wasi_proc_spawn2("missing-process", "", "", &op, 1, 0, 0, 0, "", &pid);
  assert(err == __WASI_ERRNO_BADF);
}
