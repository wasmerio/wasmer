//#ExpectedStdout: bound UDP getpeername returns ENOTCONN
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  int fd = socket(AF_INET, SOCK_DGRAM, 0);
  if (fd < 0) {
    perror("socket");
    return 1;
  }

  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_port = htons(0);
  if (inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr) != 1) {
    fprintf(stderr, "inet_pton failed\n");
    close(fd);
    return 1;
  }

  if (bind(fd, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
    perror("bind");
    close(fd);
    return 1;
  }

  struct sockaddr_in peer;
  socklen_t peer_len = sizeof(peer);
  if (getpeername(fd, (struct sockaddr*)&peer, &peer_len) == 0) {
    fprintf(stderr,
            "getpeername succeeded on bound-but-unconnected UDP socket\n");
    close(fd);
    return 1;
  }

  if (errno != ENOTCONN) {
    fprintf(stderr, "expected ENOTCONN, got errno=%d (%s)\n", errno,
            strerror(errno));
    close(fd);
    return 1;
  }

  struct sockaddr_in local;
  socklen_t local_len = sizeof(local);
  if (getsockname(fd, (struct sockaddr*)&local, &local_len) < 0) {
    perror("getsockname");
    close(fd);
    return 1;
  }

  if (ntohs(local.sin_port) == 0) {
    fprintf(stderr, "expected bound local port after failed getpeername\n");
    close(fd);
    return 1;
  }

  close(fd);
  printf("bound UDP getpeername returns ENOTCONN\n");
  return 0;
}
