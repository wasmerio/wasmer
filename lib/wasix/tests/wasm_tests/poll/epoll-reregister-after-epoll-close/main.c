//#ExpectedStdout: 0
// Registering an object in a second epoll instance, after the first one that
// watched it has closed, replaces the first one's readiness callback. That
// must neither hang nor leave the new registration deaf.
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
  int p[2];
  assert(pipe(p) == 0);

  int first = epoll_create1(0);
  assert(first >= 0);
  assert(ctl(first, EPOLL_CTL_ADD, p[0], EPOLLIN) == 0);
  assert(close(first) == 0);

  int second = epoll_create1(0);
  assert(second >= 0);
  assert(ctl(second, EPOLL_CTL_ADD, p[0], EPOLLIN) == 0);
  assert(write(p[1], "x", 1) == 1);
  assert(ready(second, 1000) == 1);

  printf("0");
  return 0;
}
