//#ExpectedStdout: socketpair flags test passed
//#MinimalLibc: v2026-10-02.1

// socketpair() must honor SOCK_CLOEXEC and SOCK_NONBLOCK like socket() does,
// and a SOCK_CLOEXEC end must be closed across exec. libuv creates duplex
// child stdio channels this way and relies on CLOEXEC to keep the parent's
// ends out of the child.

#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

static int fd_flags(int fd) {
  int flags = fcntl(fd, F_GETFD);
  assert(flags != -1);
  return flags;
}

static int status_flags(int fd) {
  int flags = fcntl(fd, F_GETFL);
  assert(flags != -1);
  return flags;
}

static int child_verify(char** fds) {
  for (int i = 0; i < 2; i++) {
    int fd = atoi(fds[i]);
    if (fcntl(fd, F_GETFD) != -1 || errno != EBADF) {
      fprintf(stderr, "child: SOCK_CLOEXEC fd %d survived exec\n", fd);
      return 1;
    }
  }
  for (int i = 2; i < 4; i++) {
    int fd = atoi(fds[i]);
    if (fcntl(fd, F_GETFD) == -1) {
      fprintf(stderr, "child: inherited fd %d is not open (errno=%d)\n", fd,
              errno);
      return 1;
    }
  }
  return 0;
}

static void test_flags_applied(int sv[2]) {
  for (int i = 0; i < 2; i++) {
    assert((fd_flags(sv[i]) & FD_CLOEXEC) != 0);
    assert((status_flags(sv[i]) & O_NONBLOCK) != 0);
  }
}

static void test_nonblocking_read(int sv[2]) {
  // The pair is empty, so a nonblocking read must fail instead of waiting.
  char buf;
  for (int i = 0; i < 2; i++) {
    errno = 0;
    assert(read(sv[i], &buf, 1) == -1);
    assert(errno == EAGAIN || errno == EWOULDBLOCK);
  }
}

static void test_no_flags(int plain[2]) {
  for (int i = 0; i < 2; i++) {
    assert((fd_flags(plain[i]) & FD_CLOEXEC) == 0);
    assert((status_flags(plain[i]) & O_NONBLOCK) == 0);
  }
}

static void test_cloexec_across_exec(int sv[2], int plain[2]) {
  char fds[4][16];
  snprintf(fds[0], sizeof fds[0], "%d", sv[0]);
  snprintf(fds[1], sizeof fds[1], "%d", sv[1]);
  snprintf(fds[2], sizeof fds[2], "%d", plain[0]);
  snprintf(fds[3], sizeof fds[3], "%d", plain[1]);
  char* spawn_argv[] = {"main", "verify", fds[0], fds[1], fds[2], fds[3], NULL};

  pid_t pid = 0;
  assert(posix_spawn(&pid, "./main", NULL, NULL, spawn_argv, NULL) == 0);

  int status = 0;
  assert(waitpid(pid, &status, 0) == pid);
  assert(WIFEXITED(status));
  assert(WEXITSTATUS(status) == 0);
}

int main(int argc, char** argv) {
  if (argc == 6 && strcmp(argv[1], "verify") == 0) {
    return child_verify(&argv[2]);
  }

  int sv[2];
  assert(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0,
                    sv) == 0);
  int plain[2];
  assert(socketpair(AF_UNIX, SOCK_STREAM, 0, plain) == 0);

  test_flags_applied(sv);
  test_nonblocking_read(sv);
  test_no_flags(plain);
  test_cloexec_across_exec(sv, plain);

  for (int i = 0; i < 2; i++) {
    assert(close(sv[i]) == 0);
    assert(close(plain[i]) == 0);
  }

  printf("socketpair flags test passed\n");
  return 0;
}
