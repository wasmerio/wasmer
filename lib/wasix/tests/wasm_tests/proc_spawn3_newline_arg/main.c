#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

static int child_verify(int argc, char** argv) {
  assert(argc >= 3);
  assert(strcmp(argv[2], "line1\nline2") == 0);
  return 0;
}

int main(int argc, char** argv) {
  if (argc >= 2 && strcmp(argv[1], "verify") == 0) {
    return child_verify(argc, argv);
  }

  const char* spawn_argv[] = {"main", "verify", "line1\nline2"};
  const uint8_t** spawn_argv_ptrs = (const uint8_t**)spawn_argv;

  __wasi_pid_t pid = 0;
  __wasi_errno_t err =
      __wasi_proc_spawn3("./main", spawn_argv_ptrs, 3, NULL, 0, NULL, 0, NULL,
                         0, __WASI_BOOL_FALSE, "", &pid);
  assert(err == __WASI_ERRNO_SUCCESS);
  assert(pid > 0);

  int status = 0;
  assert(waitpid(pid, &status, 0) == pid);
  assert(WIFEXITED(status));
  assert(WEXITSTATUS(status) == 0);

  printf("proc_spawn3 newline arg test passed\n");
  return 0;
}
