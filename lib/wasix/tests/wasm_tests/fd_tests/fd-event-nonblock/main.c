#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <sys/eventfd.h>
#include <unistd.h>

/* EFD_NONBLOCK must be honored on the eventfd returned by eventfd().

   POSIX/Linux eventfd(2): a read() of an eventfd whose counter is 0 fails with
   EAGAIN when the fd is non-blocking, and blocks otherwise. */
int main(void) {
  int efd = eventfd(0, EFD_NONBLOCK);
  assert(efd >= 0);

  /* The flag must be observable through F_GETFL. Checked before the read so a
     regressed (blocking) fd fails here instead of hanging on the read below. */
  int flags = fcntl(efd, F_GETFL, 0);
  assert(flags >= 0);
  assert(flags & O_NONBLOCK);

  /* counter == 0 on a non-blocking fd: read must fail with EAGAIN, not block.
   */
  uint64_t val = 0;
  errno = 0;
  ssize_t n = read(efd, &val, sizeof(val));
  assert(n == -1);
  assert(errno == EAGAIN);

  /* Control: once the counter is non-zero the same non-blocking fd reads the
     value and drains the counter, exactly as a blocking fd would. */
  uint64_t one = 1;
  assert(write(efd, &one, sizeof(one)) == sizeof(one));
  assert(read(efd, &val, sizeof(val)) == sizeof(val));
  assert(val == 1);

  close(efd);
}
