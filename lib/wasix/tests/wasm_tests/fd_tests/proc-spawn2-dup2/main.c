//#ExpectedStdout: proc_spawn2 dup2 test passed
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <spawn.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

#define TARGET_FD 10

static const char* src_path = "proc_spawn2_dup2_src";
static const char* decoy_path = "proc_spawn2_dup2_decoy";

static int child_verify(void) {
  char value = 0;
  if (pread(TARGET_FD, &value, 1, 0) != 1) {
    fprintf(stderr, "child: pread(TARGET_FD) failed errno=%d\n", errno);
    return 1;
  }
  if (value != 'S') {
    fprintf(stderr, "child: expected 'S' from proc_spawn2 dup2, got '%c'\n",
            value);
    return 1;
  }
  return 0;
}

static void test_invalid_src_fd(void) {
  int src_fd = open(src_path, O_RDONLY);
  assert(src_fd >= 0);

  __wasi_proc_spawn_fd_op_t op = {0};
  op.cmd = __WASI_PROC_SPAWN_FD_OP_NAME_DUP2;
  op.fd = TARGET_FD;
  op.src_fd = 99999;

  __wasi_pid_t pid = 0;
  __wasi_errno_t err = __wasi_proc_spawn2("./main", "main\nverify\n", "", &op,
                                          1, 0, 0, 0, "", &pid);
  assert(err == __WASI_ERRNO_BADF);

  assert(close(src_fd) == 0);
}

static void test_dup2_replaces_occupied_target(void) {
  unlink(decoy_path);
  int decoy_fd = open(decoy_path, O_CREAT | O_TRUNC | O_RDWR, 0644);
  assert(decoy_fd >= 0);
  assert(write(decoy_fd, "X", 1) == 1);
  if (decoy_fd != TARGET_FD) {
    assert(dup2(decoy_fd, TARGET_FD) == TARGET_FD);
    assert(close(decoy_fd) == 0);
  }

  char before = 0;
  assert(pread(TARGET_FD, &before, 1, 0) == 1);
  assert(before == 'X');

  int src_fd = open(src_path, O_RDONLY);
  assert(src_fd >= 0);

  posix_spawn_file_actions_t fdops;
  assert(posix_spawn_file_actions_init(&fdops) == 0);
  assert(posix_spawn_file_actions_adddup2(&fdops, src_fd, TARGET_FD) == 0);

  char* spawn_argv[] = {"main", "verify", NULL};
  pid_t pid = 0;
  assert(posix_spawn(&pid, "./main", &fdops, NULL, spawn_argv, NULL) == 0);
  assert(posix_spawn_file_actions_destroy(&fdops) == 0);

  int status = 0;
  assert(waitpid(pid, &status, 0) == pid);
  assert(WIFEXITED(status));
  assert(WEXITSTATUS(status) == 0);

  assert(close(src_fd) == 0);
}

static int parent_main(void) {
  unlink(src_path);
  int seed = open(src_path, O_CREAT | O_TRUNC | O_RDWR, 0644);
  assert(seed >= 0);
  assert(write(seed, "S", 1) == 1);
  assert(close(seed) == 0);

  test_invalid_src_fd();
  test_dup2_replaces_occupied_target();

  assert(unlink(src_path) == 0);
  assert(unlink(decoy_path) == 0);

  printf("proc_spawn2 dup2 test passed\n");
  return 0;
}

int main(int argc, char** argv) {
  if (argc >= 2 && strcmp(argv[1], "verify") == 0) {
    return child_verify();
  }
  return parent_main();
}
