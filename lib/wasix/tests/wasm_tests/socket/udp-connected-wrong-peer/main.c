//#ExpectedStdout: connected UDP ignores wrong peer datagrams

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int bind_loopback(int fd, struct sockaddr_in* out) {
  memset(out, 0, sizeof(*out));
  out->sin_family = AF_INET;
  out->sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  out->sin_port = 0;
  if (bind(fd, (struct sockaddr*)out, sizeof(*out)) != 0) {
    perror("bind");
    return -1;
  }
  socklen_t len = sizeof(*out);
  if (getsockname(fd, (struct sockaddr*)out, &len) != 0) {
    perror("getsockname");
    return -1;
  }
  return 0;
}

int main(void) {
  int peer_fd = socket(AF_INET, SOCK_DGRAM, 0);
  int conn_fd = socket(AF_INET, SOCK_DGRAM, 0);
  int stranger_fd = socket(AF_INET, SOCK_DGRAM, 0);
  if (peer_fd < 0 || conn_fd < 0 || stranger_fd < 0) {
    perror("socket");
    return 1;
  }

  struct sockaddr_in peer_addr;
  struct sockaddr_in conn_addr;
  if (bind_loopback(peer_fd, &peer_addr) != 0 ||
      bind_loopback(conn_fd, &conn_addr) != 0) {
    return 1;
  }

  if (connect(conn_fd, (struct sockaddr*)&peer_addr, sizeof(peer_addr)) != 0) {
    perror("connect");
    return 1;
  }

  if (fcntl(conn_fd, F_SETFL, fcntl(conn_fd, F_GETFL, 0) | O_NONBLOCK) != 0) {
    perror("fcntl(O_NONBLOCK)");
    return 1;
  }

  const char bad[] = "bad";
  if (sendto(stranger_fd, bad, sizeof(bad) - 1, 0, (struct sockaddr*)&conn_addr,
             sizeof(conn_addr)) != (ssize_t)(sizeof(bad) - 1)) {
    perror("sendto(stranger)");
    return 1;
  }

  struct pollfd pfd = {.fd = conn_fd, .events = POLLIN};
  int poll_ret = poll(&pfd, 1, 0);
  if (poll_ret != 0) {
    fprintf(stderr, "poll after wrong peer: expected 0, got %d revents=0x%x\n",
            poll_ret, pfd.revents);
    return 1;
  }

  char buf[16];
  if (recv(conn_fd, buf, sizeof(buf), 0) >= 0 ||
      (errno != EAGAIN && errno != EWOULDBLOCK)) {
    fprintf(stderr,
            "recv after wrong peer: expected EAGAIN, got errno=%d (%s)\n",
            errno, strerror(errno));
    return 1;
  }

  if (recvfrom(conn_fd, buf, sizeof(buf), 0, NULL, NULL) >= 0 ||
      (errno != EAGAIN && errno != EWOULDBLOCK)) {
    fprintf(stderr,
            "recvfrom after wrong peer: expected EAGAIN, got errno=%d (%s)\n",
            errno, strerror(errno));
    return 1;
  }

  const char good[] = "ok";
  if (sendto(peer_fd, good, sizeof(good) - 1, 0, (struct sockaddr*)&conn_addr,
             sizeof(conn_addr)) != (ssize_t)(sizeof(good) - 1)) {
    perror("sendto(peer)");
    return 1;
  }

  pfd.revents = 0;
  if (poll(&pfd, 1, 1000) != 1 || (pfd.revents & POLLIN) == 0) {
    fprintf(stderr, "poll after good peer: expected POLLIN, revents=0x%x\n",
            pfd.revents);
    return 1;
  }

  struct sockaddr_in from;
  socklen_t from_len = sizeof(from);
  ssize_t nread = recvfrom(conn_fd, buf, sizeof(buf), 0,
                           (struct sockaddr*)&from, &from_len);
  if (nread != (ssize_t)(sizeof(good) - 1) ||
      memcmp(buf, good, sizeof(good) - 1) != 0) {
    fprintf(stderr, "expected `%.*s`, got %zd bytes\n", (int)(sizeof(good) - 1),
            good, nread);
    return 1;
  }
  if (from.sin_port != peer_addr.sin_port ||
      from.sin_addr.s_addr != peer_addr.sin_addr.s_addr) {
    fprintf(stderr, "unexpected recvfrom source address\n");
    return 1;
  }

  close(stranger_fd);
  close(conn_fd);
  close(peer_fd);
  puts("connected UDP ignores wrong peer datagrams");
  return 0;
}
