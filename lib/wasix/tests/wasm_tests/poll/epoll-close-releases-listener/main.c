//#ExpectedStdout: 0
#include <arpa/inet.h>
#include <assert.h>
#include <stdio.h>
#include <sys/epoll.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  struct sockaddr_in addr = {.sin_family = AF_INET,
                             .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
  for (int round = 0; round < 3; round++) {
    int listener = socket(AF_INET, SOCK_STREAM, 0);
    assert(listener >= 0);
    assert(bind(listener, (struct sockaddr*)&addr, sizeof(addr)) == 0);
    assert(listen(listener, 1) == 0);
    socklen_t addr_len = sizeof(addr);
    assert(getsockname(listener, (struct sockaddr*)&addr, &addr_len) == 0);
    int epoll_fd = epoll_create1(0);
    assert(epoll_fd >= 0);
    struct epoll_event event = {.events = EPOLLIN, .data.fd = listener};
    assert(epoll_ctl(epoll_fd, EPOLL_CTL_ADD, listener, &event) == 0);
    // Abrupt process cleanup closes descriptors without EPOLL_CTL_DEL. Exercise
    // both close orders; a callback ownership cycle leaves the port occupied.
    if (round % 2 == 0) {
      assert(close(epoll_fd) == 0);
      assert(close(listener) == 0);
    } else {
      assert(close(listener) == 0);
      assert(close(epoll_fd) == 0);
    }
  }
  printf("0");
  return 0;
}
