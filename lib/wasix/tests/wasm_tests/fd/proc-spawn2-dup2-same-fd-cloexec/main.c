//#Config: libc
//#Args: libc
//#ExpectedStdout: proc_spawn2 same-fd dup2 test passed
//
//#Config: raw
//#Args: raw
//#ExpectedStdout: proc_spawn2 same-fd dup2 test passed

#include <errno.h>
#include <fcntl.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

#define DISTINCT_FD 40

enum spawn_api { SPAWN_LIBC, SPAWN_RAW };

static const char* source_path = "proc_spawn2_same_fd_source";
static const char* decoy_path = "proc_spawn2_same_fd_decoy";

static int check_flags(int fd, int expected) {
  int flags = fcntl(fd, F_GETFD, 0);
  if (flags < 0) {
    fprintf(stderr, "fcntl(F_GETFD, %d) failed: %s\n", fd, strerror(errno));
    return -1;
  }
  if ((flags & FD_CLOEXEC) != expected) {
    fprintf(stderr, "fd %d CLOEXEC was %d, expected %d\n", fd,
            flags & FD_CLOEXEC, expected);
    return -1;
  }
  return 0;
}

static int child_main(int argc, char** argv) {
  if (argc != 4) {
    fprintf(stderr, "child received %d arguments\n", argc);
    return 1;
  }

  int fd = atoi(argv[2]);
  if (check_flags(fd, 0) != 0) {
    return 2;
  }

  char value = 0;
  if (pread(fd, &value, 1, 0) != 1) {
    fprintf(stderr, "child pread(%d) failed: %s\n", fd, strerror(errno));
    return 3;
  }
  if (value != argv[3][0]) {
    fprintf(stderr, "child read %c from fd %d, expected %c\n", value, fd,
            argv[3][0]);
    return 4;
  }
  return 0;
}

static int spawn_with_dup2(enum spawn_api api, int source, int target,
                           char expected) {
  char target_arg[16];
  snprintf(target_arg, sizeof(target_arg), "%d", target);

  pid_t pid = 0;
  if (api == SPAWN_LIBC) {
    posix_spawn_file_actions_t actions;
    int err = posix_spawn_file_actions_init(&actions);
    if (err != 0) {
      fprintf(stderr, "posix_spawn_file_actions_init failed: %s\n",
              strerror(err));
      return -1;
    }
    err = posix_spawn_file_actions_adddup2(&actions, source, target);
    if (err == 0) {
      char expected_arg[] = {expected, '\0'};
      char* child_argv[] = {"main", "child", target_arg, expected_arg, NULL};
      err = posix_spawn(&pid, "./main", &actions, NULL, child_argv, NULL);
    }
    posix_spawn_file_actions_destroy(&actions);
    if (err != 0) {
      fprintf(stderr, "posix_spawn dup2(%d, %d) failed: %s\n", source, target,
              strerror(err));
      return -1;
    }
  } else {
    __wasi_proc_spawn_fd_op_t op = {0};
    op.cmd = __WASI_PROC_SPAWN_FD_OP_NAME_DUP2;
    op.fd = target;
    op.src_fd = source;

    char args[64];
    int args_len =
        snprintf(args, sizeof(args), "main\nchild\n%d\n%c\n", target, expected);
    if (args_len < 0 || (size_t)args_len >= sizeof(args)) {
      fputs("failed to format raw spawn arguments\n", stderr);
      return -1;
    }

    __wasi_pid_t raw_pid = 0;
    __wasi_errno_t err =
        __wasi_proc_spawn2("./main", args, "", &op, 1, 0, 0, 0, "", &raw_pid);
    if (err != __WASI_ERRNO_SUCCESS) {
      fprintf(stderr, "proc_spawn2 dup2(%d, %d) failed: %u\n", source, target,
              (unsigned)err);
      return -1;
    }
    pid = (pid_t)raw_pid;
  }

  int status = 0;
  if (waitpid(pid, &status, 0) != pid) {
    fprintf(stderr, "waitpid failed: %s\n", strerror(errno));
    return -1;
  }
  if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
    fprintf(stderr, "child exited with status %d\n", status);
    return -1;
  }
  return 0;
}

static int parent_main(enum spawn_api api) {
  unlink(source_path);
  unlink(decoy_path);

  int source = open(source_path, O_CREAT | O_TRUNC | O_RDWR | O_CLOEXEC, 0600);
  if (source < 3 || write(source, "S", 1) != 1) {
    fprintf(stderr, "failed to create source fd: %s\n", strerror(errno));
    return 1;
  }

  if (dup2(source, source) != source || check_flags(source, FD_CLOEXEC) != 0) {
    fputs("ordinary same-fd dup2 changed descriptor flags\n", stderr);
    return 2;
  }
  if (spawn_with_dup2(api, source, source, 'S') != 0) {
    fputs("same-fd spawn action failed\n", stderr);
    return 3;
  }
  if (check_flags(source, FD_CLOEXEC) != 0) {
    fputs("same-fd spawn action changed the parent descriptor\n", stderr);
    return 3;
  }

  if (__wasi_fd_fdflags_set(source, 0) != __WASI_ERRNO_SUCCESS ||
      check_flags(source, 0) != 0) {
    fputs("failed to clear CLOEXEC for the control case\n", stderr);
    return 4;
  }
  if (spawn_with_dup2(api, source, source, 'S') != 0 ||
      check_flags(source, 0) != 0) {
    fputs("same-fd spawn action failed for an already-clear descriptor\n",
          stderr);
    return 4;
  }

  if (__wasi_fd_fdflags_set(source, __WASI_FDFLAGSEXT_CLOEXEC) !=
      __WASI_ERRNO_SUCCESS) {
    fprintf(stderr, "failed to restore source CLOEXEC: %s\n", strerror(errno));
    return 5;
  }
  int decoy = open(decoy_path, O_CREAT | O_TRUNC | O_RDWR, 0600);
  if (decoy < 0 || write(decoy, "D", 1) != 1 ||
      dup2(decoy, DISTINCT_FD) != DISTINCT_FD) {
    fprintf(stderr, "failed to create distinct target fd: %s\n",
            strerror(errno));
    return 6;
  }
  close(decoy);

  if (spawn_with_dup2(api, source, DISTINCT_FD, 'S') != 0) {
    fputs("distinct-fd spawn action failed\n", stderr);
    return 7;
  }
  if (check_flags(source, FD_CLOEXEC) != 0) {
    fputs("distinct-fd spawn action changed the parent source\n", stderr);
    return 7;
  }
  char parent_value = 0;
  if (pread(DISTINCT_FD, &parent_value, 1, 0) != 1 || parent_value != 'D') {
    fputs("distinct-fd spawn action changed the parent target\n", stderr);
    return 8;
  }

  close(DISTINCT_FD);
  close(source);
  unlink(source_path);
  unlink(decoy_path);
  puts("proc_spawn2 same-fd dup2 test passed");
  return 0;
}

int main(int argc, char** argv) {
  if (argc >= 2 && strcmp(argv[1], "child") == 0) {
    return child_main(argc, argv);
  }
  if (argc != 2) {
    fputs("expected libc or raw argument\n", stderr);
    return 1;
  }
  if (strcmp(argv[1], "libc") == 0) {
    return parent_main(SPAWN_LIBC);
  }
  if (strcmp(argv[1], "raw") == 0) {
    return parent_main(SPAWN_RAW);
  }
  fputs("unknown spawn API\n", stderr);
  return 1;
}
