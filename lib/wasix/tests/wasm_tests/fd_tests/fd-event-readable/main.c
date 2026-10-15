#include <assert.h>
#include <poll.h>
#include <stdint.h>
#include <sys/eventfd.h>
#include <unistd.h>

/* Poll the fd with timeout=0 (non-blocking). Returns the revents mask, or 0 if
   poll() returned no events. Fails the test if poll() itself errors. */
static short poll_events(int fd) {
  struct pollfd pfd = {.fd = fd, .events = POLLIN};
  int poll_result = poll(&pfd, 1, 0);

  assert(poll_result == 0 || poll_result == 1);

  return poll_result == 1 ? pfd.revents : 0;
}

int main(void) {
  int efd = eventfd(0, 0);
  assert(efd >= 0);

  /* counter=0: must not be readable and must not look like a hangup */
  assert(poll_events(efd) == 0);

  /* write 1 -> counter becomes 1: must be readable */
  uint64_t val = 1;
  assert(write(efd, &val, sizeof(val)) == sizeof(val));
  assert(poll_events(efd) & POLLIN);

  /* still readable without an intervening read */
  assert(poll_events(efd) & POLLIN);

  /* read drains the counter, must not be readable afterwards */
  uint64_t result;
  assert(read(efd, &result, sizeof(result)) == sizeof(result));
  assert(result == 1);
  assert(poll_events(efd) == 0);

  close(efd);
}
