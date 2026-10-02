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

static int child_verify(int cloexec_fd, int inherited_fd) {
  if (fcntl(cloexec_fd, F_GETFD) != -1 || errno != EBADF) {
    fprintf(stderr, "child: SOCK_CLOEXEC fd %d survived exec\n", cloexec_fd);
    return 1;
  }
  if (fcntl(inherited_fd, F_GETFD) == -1) {
    fprintf(stderr, "child: inherited fd %d is not open (errno=%d)\n",
            inherited_fd, errno);
    return 1;
  }
  return 0;
}

static void test_flags_applied(int sv[2]) {
  for (int i = 0; i < 2; i++) {
    assert((fcntl(sv[i], F_GETFD) & FD_CLOEXEC) != 0);
    assert((fcntl(sv[i], F_GETFL) & O_NONBLOCK) != 0);
  }
}

static void test_no_flags(int plain[2]) {
  for (int i = 0; i < 2; i++) {
    assert((fcntl(plain[i], F_GETFD) & FD_CLOEXEC) == 0);
    assert((fcntl(plain[i], F_GETFL) & O_NONBLOCK) == 0);
  }
}

static void test_cloexec_across_exec(int cloexec_fd, int inherited_fd) {
  char a[16], b[16];
  snprintf(a, sizeof a, "%d", cloexec_fd);
  snprintf(b, sizeof b, "%d", inherited_fd);
  char* spawn_argv[] = {"main", "verify", a, b, NULL};

  pid_t pid = 0;
  assert(posix_spawn(&pid, "./main", NULL, NULL, spawn_argv, NULL) == 0);

  int status = 0;
  assert(waitpid(pid, &status, 0) == pid);
  assert(WIFEXITED(status));
  assert(WEXITSTATUS(status) == 0);
}

int main(int argc, char** argv) {
  if (argc == 4 && strcmp(argv[1], "verify") == 0) {
    return child_verify(atoi(argv[2]), atoi(argv[3]));
  }

  int sv[2];
  assert(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0,
                    sv) == 0);
  int plain[2];
  assert(socketpair(AF_UNIX, SOCK_STREAM, 0, plain) == 0);

  test_flags_applied(sv);
  test_no_flags(plain);
  test_cloexec_across_exec(sv[0], plain[0]);

  for (int i = 0; i < 2; i++) {
    assert(close(sv[i]) == 0);
    assert(close(plain[i]) == 0);
  }

  printf("socketpair flags test passed\n");
  return 0;
}
