//#ExpectedStdout: 0
#include <arpa/inet.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <stdio.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  unlink("victim.php");

  // Build a real TCP loopback connection. The accepted socket is the fd watched
  // by epoll, matching the socket lifecycle used by HTTP servers.
  int listener = socket(AF_INET, SOCK_STREAM, 0);
  struct sockaddr_in addr = {.sin_family = AF_INET,
                             .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
  bind(listener, (struct sockaddr*)&addr, sizeof(addr));
  listen(listener, 1);
  socklen_t addr_len = sizeof(addr);
  getsockname(listener, (struct sockaddr*)&addr, &addr_len);
  int client = socket(AF_INET, SOCK_STREAM, 0);
  connect(client, (struct sockaddr*)&addr, sizeof(addr));
  int fd = accept(listener, NULL, NULL);
  int epoll_fd = epoll_create1(0);

  // Watch the socket for both read and write readiness, then make it readable.
  // Correct epoll behavior is to report one event with EPOLLIN | EPOLLOUT.
  struct epoll_event event = {.events = EPOLLIN | EPOLLOUT, .data.fd = fd};
  epoll_ctl(epoll_fd, EPOLL_CTL_ADD, fd, &event);
  write(client, "x", 1);
  usleep(10000);

  // The buggy WASIX implementation split those readiness bits into multiple
  // events for the same fd. If we get fewer than two events, the fix is
  // working.
  struct epoll_event events[8];
  int n = epoll_wait(epoll_fd, events, 8, 1000);
  if (n < 2) {
    printf("0");
    return 0;
  }

  // Simulate the app handling the first event by closing the socket. The next
  // file open should reuse the same numeric fd, creating the stale-fd hazard.
  close(fd);
  int victim_fd = open("victim.php", O_CREAT | O_TRUNC | O_WRONLY, 0600);
  if (victim_fd != fd) return 2;

  // A stale second epoll event still carries the old fd number. With the bug,
  // writing through that stale event writes into victim.php instead of a
  // socket.
  write(victim_fd, "<?php echo 'ok'; ?>\n", 19);
  write(events[1].data.fd, "TLS_HANDSHAKE_BYTES", 19);
  close(victim_fd);

  // The test passes only when the stale socket bytes never appear in the file.
  char buf[256] = {0};
  victim_fd = open("victim.php", O_RDONLY);
  read(victim_fd, buf, sizeof(buf) - 1);
  if (strstr(buf, "TLS_HANDSHAKE_BYTES") != NULL) return 1;
  printf("0");
  return 0;
}
