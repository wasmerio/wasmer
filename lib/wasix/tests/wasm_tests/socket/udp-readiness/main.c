//#MinimalLibc: v2026-06-18.1
//#ExpectedStdout: received datagram length: 0

#include <arpa/inet.h>
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  int fd = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);

  struct sockaddr_in addr = {0};
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr.sin_port = 0;
  bind(fd, (struct sockaddr*)&addr, sizeof(addr));

  socklen_t len = sizeof(addr);
  getsockname(fd, (struct sockaddr*)&addr, &len);

  sendto(fd, "", 0, 0, (struct sockaddr*)&addr, sizeof(addr));

  struct pollfd pfd = {.fd = fd, .events = POLLIN};
  poll(&pfd, 1, 1000);

  char byte;
  ssize_t n = recvfrom(fd, &byte, sizeof(byte), 0, NULL, NULL);
  close(fd);

  if (n < 0) {
    perror("recvfrom after poll");
    return 1;
  }

  printf("received datagram length: %zd\n", n);
  return 0;
}