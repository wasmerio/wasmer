//#ExpectedStdout: fcntl F_SETFD test passed
//#MinimalLibc: v2026-10-02.1

// fcntl(F_SETFD) must both set and clear FD_CLOEXEC; the libc used to treat
// every F_SETFD as a request to set it.

#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

static int fd_flags(int fd) {
  int flags = fcntl(fd, F_GETFD);
  assert(flags != -1);
  return flags;
}

static void test_set_and_clear(int fd) {
  assert((fd_flags(fd) & FD_CLOEXEC) != 0);

  assert(fcntl(fd, F_SETFD, 0) == 0);
  assert((fd_flags(fd) & FD_CLOEXEC) == 0);

  assert(fcntl(fd, F_SETFD, FD_CLOEXEC) == 0);
  assert((fd_flags(fd) & FD_CLOEXEC) != 0);
}

static void test_per_descriptor(int fd) {
  // dup() shares the open file description but not the descriptor flags:
  // the copy starts without CLOEXEC, and changing one leaves the other alone.
  int copy = dup(fd);
  assert(copy != -1);
  assert((fd_flags(fd) & FD_CLOEXEC) != 0);
  assert((fd_flags(copy) & FD_CLOEXEC) == 0);

  assert(fcntl(copy, F_SETFD, FD_CLOEXEC) == 0);
  assert(fcntl(fd, F_SETFD, 0) == 0);
  assert((fd_flags(copy) & FD_CLOEXEC) != 0);
  assert((fd_flags(fd) & FD_CLOEXEC) == 0);

  assert(close(copy) == 0);

  // F_DUPFD_CLOEXEC sets the flag on the copy only.
  copy = fcntl(fd, F_DUPFD_CLOEXEC, 0);
  assert(copy != -1);
  assert((fd_flags(copy) & FD_CLOEXEC) != 0);
  assert((fd_flags(fd) & FD_CLOEXEC) == 0);

  assert(close(copy) == 0);
}

int main(void) {
  int p[2];
  assert(pipe2(p, O_CLOEXEC) == 0);

  test_set_and_clear(p[0]);
  test_per_descriptor(p[0]);

  assert(close(p[0]) == 0);
  assert(close(p[1]) == 0);

  printf("fcntl F_SETFD test passed\n");
  return 0;
}
