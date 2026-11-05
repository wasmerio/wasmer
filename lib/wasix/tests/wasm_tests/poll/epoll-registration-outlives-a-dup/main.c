//#ExpectedStdout: 0
// A registration belongs to the open file description, not to one descriptor
// number (epoll(7)): closing one of two descriptors for the same object keeps
// it, and so does closing an epoll descriptor while a dup of it remains.
#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/epoll.h>
#include <sys/eventfd.h>
#include <unistd.h>

// wasix-libc's epoll_ctl hands back the errno itself on failure, rather than -1
// with errno set, so only "is it zero" is portable here.
static int ctl(int ep, int op, int fd, uint32_t events) {
  struct epoll_event event = {.events = events, .data.fd = fd};
  int rc = epoll_ctl(ep, op, fd, &event);
  return rc == -1 ? errno : rc;
}

static int ready(int ep, int timeout_ms) {
  struct epoll_event events[4];
  return epoll_wait(ep, events, 4, timeout_ms);
}

static void signal_event(int fd) {
  uint64_t one = 1;
  assert(write(fd, &one, sizeof(one)) == sizeof(one));
}

int main(void) {
  // The watched object stays registered while a dup of it is open, and its
  // readiness is still reported.
  int ep = epoll_create1(0);
  int efd = eventfd(0, 0);
  // Allocated now, so that it does not take efd's number once efd closes: the
  // registration the dup keeps alive is still filed under that number, and
  // WASIX keys interest lists by number alone, where Linux keys them by number
  // and open file together.
  int other = eventfd(0, 0);
  assert(ep >= 0 && efd >= 0 && other >= 0);
  assert(ctl(ep, EPOLL_CTL_ADD, efd, EPOLLIN) == 0);
  int efd_dup = dup(efd);
  assert(efd_dup >= 0);
  assert(close(efd) == 0);
  signal_event(efd_dup);
  assert(ready(ep, 1000) == 1);

  // The epoll instance keeps its interest list while a dup of it is open.
  assert(ctl(ep, EPOLL_CTL_ADD, other, EPOLLIN) == 0);
  int ep_dup = dup(ep);
  assert(ep_dup >= 0);
  assert(close(ep) == 0);
  signal_event(other);
  assert(ready(ep_dup, 1000) >= 1);

  printf("0");
  return 0;
}
