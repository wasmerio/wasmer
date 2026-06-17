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
  if (fd < 0) {
    perror("socket");
    return 1;
  }

  struct sockaddr_in addr = {0};
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr.sin_port = 0;
  if (bind(fd, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("bind");
    close(fd);
    return 1;
  }

  socklen_t len = sizeof(addr);
  if (getsockname(fd, (struct sockaddr*)&addr, &len) != 0) {
    perror("getsockname");
    close(fd);
    return 1;
  }

  ssize_t sent =
      sendto(fd, "", 0, 0, (struct sockaddr*)&addr, sizeof(addr));
  if (sent != 0) {
    fprintf(stderr, "expected zero-length sendto, got %zd errno=%d (%s)\n",
            sent, errno, strerror(errno));
    close(fd);
    return 1;
  }

  struct pollfd pfd = {.fd = fd, .events = POLLIN};
  int ready = poll(&pfd, 1, 1000);
  if (ready < 0) {
    perror("poll");
    close(fd);
    return 1;
  }
  if (ready == 0) {
    fprintf(stderr,
            "poll did not report an already queued zero-length UDP datagram\n");
    close(fd);
    return 1;
  }
  if ((pfd.revents & POLLIN) == 0) {
    fprintf(stderr, "poll did not report POLLIN, revents=0x%x\n", pfd.revents);
    close(fd);
    return 1;
  }

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
