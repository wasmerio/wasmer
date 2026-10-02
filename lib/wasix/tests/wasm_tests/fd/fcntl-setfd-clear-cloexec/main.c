//#ExpectedStdout: fcntl F_SETFD test passed

// fcntl(F_SETFD) must both set and clear FD_CLOEXEC; the libc used to treat
// every F_SETFD as a request to set it.

#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

int main(void) {
  int p[2];
  assert(pipe2(p, O_CLOEXEC) == 0);
  assert((fcntl(p[0], F_GETFD) & FD_CLOEXEC) != 0);

  assert(fcntl(p[0], F_SETFD, 0) == 0);
  assert((fcntl(p[0], F_GETFD) & FD_CLOEXEC) == 0);

  assert(fcntl(p[0], F_SETFD, FD_CLOEXEC) == 0);
  assert((fcntl(p[0], F_GETFD) & FD_CLOEXEC) != 0);

  /* The flag is per descriptor: clearing one end leaves the other alone. */
  assert(fcntl(p[0], F_SETFD, 0) == 0);
  assert((fcntl(p[1], F_GETFD) & FD_CLOEXEC) != 0);

  assert(close(p[0]) == 0);
  assert(close(p[1]) == 0);

  printf("fcntl F_SETFD test passed\n");
  return 0;
}
