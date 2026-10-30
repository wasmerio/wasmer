//#ExpectedStdout: 0
// Closing the last descriptor for an object removes it from every interest
// list that watches it, so its number is free for the next EPOLL_CTL_ADD and
// the object itself is released, whichever way the descriptor goes.
#include <arpa/inet.h>
#include <sys/socket.h>
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
  int ep = epoll_create1(0);
  assert(ep >= 0);

  // dup2 over a watched descriptor closes its object.
  int a = eventfd(0, 0);
  int b = eventfd(0, 0);
  assert(a >= 0 && b >= 0);
  assert(ctl(ep, EPOLL_CTL_ADD, a, EPOLLIN) == 0);
  assert(dup2(b, a) == a);
  assert(ctl(ep, EPOLL_CTL_ADD, a, EPOLLIN) == 0);
  assert(ctl(ep, EPOLL_CTL_DEL, a, 0) == 0);
  assert(close(a) == 0);
  assert(close(b) == 0);

  // A pipe's write end, which epoll registers without a readiness handler.
  int p[2];
  assert(pipe(p) == 0);
  int write_end = p[1];
  assert(ctl(ep, EPOLL_CTL_ADD, write_end, EPOLLOUT) == 0);
  assert(close(p[1]) == 0);
  assert(close(p[0]) == 0);
  int q[2];
  assert(pipe(q) == 0);
  assert(q[1] == write_end);
  assert(ctl(ep, EPOLL_CTL_ADD, q[1], EPOLLOUT) == 0);
  assert(close(q[0]) == 0 && close(q[1]) == 0);

  // A listening socket closed while its epoll instance stays open releases its
  // port.
  struct sockaddr_in addr = {.sin_family = AF_INET,
                             .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
  int listener = socket(AF_INET, SOCK_STREAM, 0);
  assert(listener >= 0);
  assert(bind(listener, (struct sockaddr*)&addr, sizeof(addr)) == 0);
  assert(listen(listener, 1) == 0);
  socklen_t addr_len = sizeof(addr);
  assert(getsockname(listener, (struct sockaddr*)&addr, &addr_len) == 0);
  assert(ctl(ep, EPOLL_CTL_ADD, listener, EPOLLIN) == 0);
  assert(close(listener) == 0);
  int again = socket(AF_INET, SOCK_STREAM, 0);
  assert(again >= 0);
  assert(bind(again, (struct sockaddr*)&addr, sizeof(addr)) == 0);
  assert(close(again) == 0);

  // Readiness queued just before the last close is not reported for the
  // descriptor the guest no longer holds.
  int efd = eventfd(0, 0);
  assert(efd >= 0);
  assert(ctl(ep, EPOLL_CTL_ADD, efd, EPOLLIN) == 0);
  signal_event(efd);
  assert(close(efd) == 0);
  assert(ready(ep, 100) == 0);

  printf("0");
  return 0;
}
