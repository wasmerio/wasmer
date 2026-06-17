//#ExpectedStdout: nonblocking cloexec socket flags are visible through POSIX APIs
//#MinimalLibc: v2026-06-03.1

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  int fd = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
  if (fd < 0) {
    perror("socket");
    return 1;
  }

  int status_flags = fcntl(fd, F_GETFL, 0);
  if (status_flags < 0) {
    perror("fcntl(F_GETFL)");
    return 1;
  }
  if ((status_flags & O_NONBLOCK) == 0) {
    fprintf(stderr, "socket is not nonblocking\n");
    return 1;
  }

  int fd_flags = fcntl(fd, F_GETFD, 0);
  if (fd_flags < 0) {
    perror("fcntl(F_GETFD)");
    return 1;
  }
  if ((fd_flags & FD_CLOEXEC) == 0) {
    fprintf(stderr, "socket is not close-on-exec\n");
    return 1;
  }

  struct sockaddr_in local;
  memset(&local, 0, sizeof(local));
  local.sin_family = AF_INET;
  local.sin_port = htons(0);
  if (inet_pton(AF_INET, "127.0.0.1", &local.sin_addr) != 1) {
    fprintf(stderr, "inet_pton failed\n");
    return 1;
  }
  if (bind(fd, (const struct sockaddr*)&local, sizeof(local)) != 0) {
    perror("bind");
    return 1;
  }

  char byte;
  struct sockaddr_storage peer;
  socklen_t peer_len = sizeof(peer);
  ssize_t nread =
      recvfrom(fd, &byte, sizeof(byte), 0, (struct sockaddr*)&peer, &peer_len);
  if (nread < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
    puts("nonblocking cloexec socket flags are visible through POSIX APIs");
    close(fd);
    return 0;
  }

  fprintf(stderr,
          "expected nonblocking recvfrom, got nread=%zd errno=%d (%s)\n", nread,
          errno, strerror(errno));
  return 1;
}
