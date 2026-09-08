//#ExpectedStdout: ST
//#ExpectedStdout: proc_spawn2 open exact target test passed
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

#define OCCUPIED_FD 10
#define EMPTY_FD 11
#define ORDERED_FD 12
#define FAILURE_FD 14

static void write_file(const char* path, char value) {
  int fd = open(path, O_CREAT | O_TRUNC | O_RDWR, 0644);
  assert(fd >= 0);
  assert(write(fd, &value, 1) == 1);
  assert(close(fd) == 0);
}

static char read_at(int fd) {
  char value = 0;
  assert(pread(fd, &value, 1, 0) == 1);
  return value;
}

static __wasi_proc_spawn_fd_op_t open_op(__wasi_fd_t fd, const char* path,
                                         __wasi_oflags_t oflags,
                                         __wasi_rights_t rights,
                                         __wasi_fdflags_t fdflags,
                                         __wasi_fdflagsext_t fdflagsext) {
  __wasi_proc_spawn_fd_op_t op = {0};
  op.cmd = __WASI_PROC_SPAWN_FD_OP_NAME_OPEN;
  op.fd = fd;
  op.path = (uint8_t*)path;
  op.path_len = strlen(path);
  op.dirflags = 0;
  op.oflags = oflags;
  op.fs_rights_base = rights;
  op.fs_rights_inheriting = rights;
  op.fdflags = fdflags;
  op.fdflagsext = fdflagsext;
  return op;
}

static __wasi_errno_t spawn(const char* child_args,
                            const __wasi_proc_spawn_fd_op_t* ops,
                            size_t ops_len, __wasi_pid_t* pid) {
  char args[128];
  int len = snprintf(args, sizeof(args), "main\n%s", child_args);
  assert(len > 0 && (size_t)len < sizeof(args));
  return __wasi_proc_spawn2("./main", args, "", ops, ops_len, 0, 0, 0, "", pid);
}

static void wait_ok(__wasi_pid_t pid) {
  int status = 0;
  assert(waitpid((pid_t)pid, &status, 0) == (pid_t)pid);
  assert(WIFEXITED(status));
  assert(WEXITSTATUS(status) == 0);
}

static int child_main(int argc, char** argv) {
  assert(argc >= 3);

  if (strcmp(argv[1], "read") == 0) {
    int fd = atoi(argv[2]);
    assert(argc == 4);
    assert(read_at(fd) == argv[3][0]);
    return 0;
  }

  if (strcmp(argv[1], "write") == 0) {
    int fd = atoi(argv[2]);
    assert(argc == 4);
    assert(write(fd, argv[3], strlen(argv[3])) == (ssize_t)strlen(argv[3]));
    return 0;
  }

  if (strcmp(argv[1], "special") == 0) {
    int fd = atoi(argv[2]);
    assert(argc == 3);
    assert(write(fd, "S", 1) == 1);
    assert(close(fd) == 0);
    errno = 0;
    assert(write(fd, "X", 1) == -1);
    assert(errno == EBADF);
    assert(write(STDOUT_FILENO, "T\n", 2) == 2);
    return 0;
  }

  return 2;
}

static void test_occupied_target(void) {
  write_file("open-source-a", 'A');
  write_file("open-decoy", 'D');

  int decoy = open("open-decoy", O_RDONLY);
  assert(decoy >= 0);
  assert(dup2(decoy, OCCUPIED_FD) == OCCUPIED_FD);
  if (decoy != OCCUPIED_FD) {
    assert(close(decoy) == 0);
  }

  __wasi_proc_spawn_fd_op_t op =
      open_op(OCCUPIED_FD, "open-source-a", 0, __WASI_RIGHTS_FD_READ, 0, 0);
  __wasi_pid_t pid = 0;
  assert(spawn("read\n10\nA\n", &op, 1, &pid) == __WASI_ERRNO_SUCCESS);
  wait_ok(pid);

  assert(read_at(OCCUPIED_FD) == 'D');
  assert(close(OCCUPIED_FD) == 0);
}

static void test_empty_target(void) {
  close(EMPTY_FD);
  __wasi_proc_spawn_fd_op_t op =
      open_op(EMPTY_FD, "open-source-a", 0, __WASI_RIGHTS_FD_READ, 0, 0);
  __wasi_pid_t pid = 0;
  assert(spawn("read\n11\nA\n", &op, 1, &pid) == __WASI_ERRNO_SUCCESS);
  wait_ok(pid);
}

static void test_ordered_replacement(void) {
  write_file("open-source-b", 'B');
  __wasi_proc_spawn_fd_op_t ops[2] = {
      open_op(ORDERED_FD, "open-source-a", 0, __WASI_RIGHTS_FD_READ, 0, 0),
      open_op(ORDERED_FD, "open-source-b", 0, __WASI_RIGHTS_FD_READ, 0, 0),
  };
  __wasi_pid_t pid = 0;
  assert(spawn("read\n12\nB\n", ops, 2, &pid) == __WASI_ERRNO_SUCCESS);
  wait_ok(pid);
}

static void test_stdio_target(void) {
  unlink("open-stdout");
  __wasi_proc_spawn_fd_op_t op = open_op(
      STDOUT_FILENO, "open-stdout", __WASI_OFLAGS_CREAT | __WASI_OFLAGS_TRUNC,
      __WASI_RIGHTS_FD_WRITE, 0, 0);
  __wasi_pid_t pid = 0;
  assert(spawn("write\n1\nredirected\n", &op, 1, &pid) == __WASI_ERRNO_SUCCESS);
  wait_ok(pid);

  int fd = open("open-stdout", O_RDONLY);
  assert(fd >= 0);
  char output[16] = {0};
  assert(read(fd, output, sizeof(output)) == 10);
  assert(strcmp(output, "redirected") == 0);
  assert(close(fd) == 0);
}

static void test_special_file_target(void) {
  close(OCCUPIED_FD);
  __wasi_proc_spawn_fd_op_t op =
      open_op(OCCUPIED_FD, "/dev/stdout", 0, __WASI_RIGHTS_FD_WRITE, 0, 0);
  __wasi_pid_t pid = 0;
  assert(spawn("special\n10\n", &op, 1, &pid) == __WASI_ERRNO_SUCCESS);
  wait_ok(pid);
}

static void test_missing_path_closes_only_child_target(void) {
  unlink("open-missing");
  write_file("open-failure-decoy", 'F');
  int decoy = open("open-failure-decoy", O_RDONLY);
  assert(decoy >= 0);
  assert(dup2(decoy, FAILURE_FD) == FAILURE_FD);
  if (decoy != FAILURE_FD) {
    assert(close(decoy) == 0);
  }

  __wasi_proc_spawn_fd_op_t op =
      open_op(FAILURE_FD, "open-missing", 0, __WASI_RIGHTS_FD_READ, 0, 0);
  __wasi_pid_t pid = 0;
  assert(spawn("read\n14\nX\n", &op, 1, &pid) == __WASI_ERRNO_NOENT);
  assert(read_at(FAILURE_FD) == 'F');
  assert(close(FAILURE_FD) == 0);
}

static void test_protected_target_has_no_side_effect(void) {
  write_file("open-protected", 'P');
  __wasi_proc_spawn_fd_op_t op = open_op(
      3, "open-protected", __WASI_OFLAGS_TRUNC, __WASI_RIGHTS_FD_WRITE, 0, 0);
  __wasi_pid_t pid = 0;
  assert(spawn("write\n3\nX\n", &op, 1, &pid) == __WASI_ERRNO_NOTSUP);

  int fd = open("open-protected", O_RDONLY);
  assert(fd >= 0);
  assert(read_at(fd) == 'P');
  assert(close(fd) == 0);
}

static void test_huge_target_has_no_side_effect(void) {
  unlink("open-huge");
  __wasi_proc_spawn_fd_op_t op = open_op(
      65536, "open-huge", __WASI_OFLAGS_CREAT, __WASI_RIGHTS_FD_WRITE, 0, 0);
  __wasi_pid_t pid = 0;
  assert(spawn("write\n65536\nX\n", &op, 1, &pid) == __WASI_ERRNO_BADF);
  assert(access("open-huge", F_OK) == -1);
  assert(errno == ENOENT);
}

int main(int argc, char** argv) {
  if (argc >= 2) {
    return child_main(argc, argv);
  }

  test_occupied_target();
  test_empty_target();
  test_ordered_replacement();
  test_stdio_target();
  test_special_file_target();
  test_missing_path_closes_only_child_target();
  test_protected_target_has_no_side_effect();
  test_huge_target_has_no_side_effect();

  unlink("open-source-a");
  unlink("open-source-b");
  unlink("open-decoy");
  unlink("open-stdout");
  unlink("open-failure-decoy");
  unlink("open-protected");
  printf("proc_spawn2 open exact target test passed\n");
  return 0;
}
