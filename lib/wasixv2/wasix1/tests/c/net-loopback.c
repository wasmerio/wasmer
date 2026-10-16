// A WASIX v1 program (wasix-libc) exercising the networking calls end to end:
// name lookup, a TCP loopback connection, poll() and epoll. Built and run by
// `tests/net.rs` (`c_loopback`) when WASIX1_TEST_CC names a v1 compiler.

#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <netinet/in.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/socket.h>
#include <unistd.h>

#define CHECK(cond)                                                        \
  do {                                                                     \
    if (!(cond)) {                                                         \
      fprintf(stderr, "%s:%d: %s failed (errno %d)\n", __FILE__, __LINE__, \
              #cond, errno);                                               \
      return 1;                                                            \
    }                                                                      \
  } while (0)

int main(void) {
  struct addrinfo hints = {0}, *res;
  hints.ai_family = AF_INET;
  hints.ai_socktype = SOCK_STREAM;
  CHECK(getaddrinfo("localhost", NULL, &hints, &res) == 0);
  char text[INET_ADDRSTRLEN];
  struct sockaddr_in* found = (struct sockaddr_in*)res->ai_addr;
  CHECK(inet_ntop(AF_INET, &found->sin_addr, text, sizeof text) != NULL);
  printf("localhost: %s\n", text);
  freeaddrinfo(res);

  int srv = socket(AF_INET, SOCK_STREAM, 0);
  CHECK(srv >= 0);
  int one = 1;
  CHECK(setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one) == 0);
  struct sockaddr_in addr = {0};
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  CHECK(bind(srv, (struct sockaddr*)&addr, sizeof addr) == 0);
  CHECK(listen(srv, 4) == 0);
  socklen_t len = sizeof addr;
  CHECK(getsockname(srv, (struct sockaddr*)&addr, &len) == 0);
  CHECK(addr.sin_port != 0);

  int cli = socket(AF_INET, SOCK_STREAM, 0);
  CHECK(cli >= 0);
  CHECK(connect(cli, (struct sockaddr*)&addr, sizeof addr) == 0);
  struct pollfd pfd = {.fd = cli, .events = POLLOUT};
  CHECK(poll(&pfd, 1, 5000) == 1 && (pfd.revents & POLLOUT));

  struct sockaddr_in peer;
  len = sizeof peer;
  int conn = accept(srv, (struct sockaddr*)&peer, &len);
  CHECK(conn >= 0);
  CHECK(peer.sin_family == AF_INET);
  CHECK(peer.sin_addr.s_addr == htonl(INADDR_LOOPBACK));
  printf("accepted\n");

  CHECK(send(cli, "hello", 5, 0) == 5);
  pfd = (struct pollfd){.fd = conn, .events = POLLIN};
  CHECK(poll(&pfd, 1, 5000) == 1 && (pfd.revents & POLLIN));
  printf("readable\n");
  char data[16] = {0};
  CHECK(recv(conn, data, sizeof data, 0) == 5);
  CHECK(send(conn, data, 5, 0) == 5);

  int ep = epoll_create1(0);
  CHECK(ep >= 0);
  struct epoll_event ev = {.events = EPOLLIN, .data.fd = cli};
  CHECK(epoll_ctl(ep, EPOLL_CTL_ADD, cli, &ev) == 0);
  struct epoll_event out[4];
  int n = epoll_wait(ep, out, 4, 5000);
  CHECK(n == 1 && out[0].data.fd == cli && (out[0].events & EPOLLIN));
  memset(data, 0, sizeof data);
  CHECK(recv(cli, data, sizeof data, 0) == 5);
  printf("echo: %s\n", data);
  printf("epoll: %d\n", n);

  CHECK(close(ep) == 0);
  CHECK(close(conn) == 0);
  CHECK(close(cli) == 0);
  CHECK(close(srv) == 0);
  printf("ok\n");
  return 0;
}
