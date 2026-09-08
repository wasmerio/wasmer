//#ExpectedStdout: getpeername reports actual TCP connection state

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int expect_not_connected(int fd, const char* state) {
  struct sockaddr_storage peer;
  socklen_t peer_len = sizeof(peer);

  errno = 0;
  if (getpeername(fd, (struct sockaddr*)&peer, &peer_len) == 0) {
    fprintf(stderr, "%s getpeername unexpectedly succeeded\n", state);
    return -1;
  }
  if (errno != ENOTCONN) {
    fprintf(stderr, "%s getpeername returned %d (%s), expected ENOTCONN\n",
            state, errno, strerror(errno));
    return -1;
  }
  return 0;
}

static void loopback_addr(int family, struct sockaddr_storage* addr,
                          socklen_t* addr_len) {
  memset(addr, 0, sizeof(*addr));
  if (family == AF_INET) {
    struct sockaddr_in* addr4 = (struct sockaddr_in*)addr;
    addr4->sin_family = AF_INET;
    addr4->sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    *addr_len = sizeof(*addr4);
  } else {
    struct sockaddr_in6* addr6 = (struct sockaddr_in6*)addr;
    addr6->sin6_family = AF_INET6;
    addr6->sin6_addr = in6addr_loopback;
    *addr_len = sizeof(*addr6);
  }
}

static in_port_t addr_port(const struct sockaddr_storage* addr) {
  if (addr->ss_family == AF_INET)
    return ((const struct sockaddr_in*)addr)->sin_port;
  return ((const struct sockaddr_in6*)addr)->sin6_port;
}

static int same_endpoint(const struct sockaddr_storage* left,
                         const struct sockaddr_storage* right) {
  if (left->ss_family != right->ss_family ||
      addr_port(left) != addr_port(right))
    return 0;
  if (left->ss_family == AF_INET) {
    const struct sockaddr_in* left4 = (const struct sockaddr_in*)left;
    const struct sockaddr_in* right4 = (const struct sockaddr_in*)right;
    return left4->sin_addr.s_addr == right4->sin_addr.s_addr;
  }
  const struct sockaddr_in6* left6 = (const struct sockaddr_in6*)left;
  const struct sockaddr_in6* right6 = (const struct sockaddr_in6*)right;
  return memcmp(&left6->sin6_addr, &right6->sin6_addr,
                sizeof(left6->sin6_addr)) == 0;
}

static int test_unconnected_states(int family) {
  int fd = socket(family, SOCK_STREAM, 0);
  if (fd < 0) {
    perror("socket");
    return -1;
  }
  if (expect_not_connected(fd, "fresh") < 0) {
    close(fd);
    return -1;
  }

  struct sockaddr_storage addr;
  socklen_t addr_len;
  loopback_addr(family, &addr, &addr_len);
  if (bind(fd, (struct sockaddr*)&addr, addr_len) < 0) {
    perror("bind");
    close(fd);
    return -1;
  }
  if (expect_not_connected(fd, "bound") < 0) {
    close(fd);
    return -1;
  }
  if (listen(fd, 1) < 0) {
    perror("listen");
    close(fd);
    return -1;
  }
  if (expect_not_connected(fd, "listener") < 0) {
    close(fd);
    return -1;
  }

  close(fd);
  return 0;
}

static int test_connected(int family) {
  int listener = socket(family, SOCK_STREAM, 0);
  if (listener < 0) {
    perror("socket(listener)");
    return -1;
  }

  struct sockaddr_storage addr;
  socklen_t addr_len;
  loopback_addr(family, &addr, &addr_len);
  if (bind(listener, (struct sockaddr*)&addr, addr_len) < 0 ||
      listen(listener, 1) < 0) {
    perror("bind/listen");
    close(listener);
    return -1;
  }
  addr_len = sizeof(addr);
  if (getsockname(listener, (struct sockaddr*)&addr, &addr_len) < 0) {
    perror("getsockname(listener)");
    close(listener);
    return -1;
  }

  int client = socket(family, SOCK_STREAM, 0);
  if (client < 0 || connect(client, (struct sockaddr*)&addr, addr_len) < 0) {
    perror("connect");
    close(listener);
    if (client >= 0) close(client);
    return -1;
  }
  int accepted = accept(listener, NULL, NULL);
  if (accepted < 0) {
    perror("accept");
    close(client);
    close(listener);
    return -1;
  }

  struct sockaddr_storage peer;
  socklen_t peer_len = sizeof(peer);
  if (getpeername(client, (struct sockaddr*)&peer, &peer_len) < 0 ||
      !same_endpoint(&peer, &addr)) {
    fprintf(stderr, "connected getpeername returned the wrong endpoint\n");
    close(accepted);
    close(client);
    close(listener);
    return -1;
  }

  close(accepted);
  close(client);
  close(listener);
  return 0;
}

static int test_failed_connect(int family) {
  int probe = socket(family, SOCK_STREAM, 0);
  if (probe < 0) {
    perror("socket(probe)");
    return -1;
  }
  struct sockaddr_storage addr;
  socklen_t addr_len;
  loopback_addr(family, &addr, &addr_len);
  if (bind(probe, (struct sockaddr*)&addr, addr_len) < 0) {
    perror("bind(probe)");
    close(probe);
    return -1;
  }
  addr_len = sizeof(addr);
  if (getsockname(probe, (struct sockaddr*)&addr, &addr_len) < 0) {
    perror("getsockname(probe)");
    close(probe);
    return -1;
  }
  int fd = socket(family, SOCK_STREAM, 0);
  if (fd < 0) {
    perror("socket(nonblocking)");
    close(probe);
    return -1;
  }
  int flags = fcntl(fd, F_GETFL, 0);
  if (flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK) < 0) {
    perror("fcntl(O_NONBLOCK)");
    close(fd);
    close(probe);
    return -1;
  }
  struct sockaddr_storage local;
  socklen_t local_len;
  loopback_addr(family, &local, &local_len);
  if (bind(fd, (struct sockaddr*)&local, local_len) < 0) {
    perror("bind(nonblocking)");
    close(fd);
    close(probe);
    return -1;
  }
  local_len = sizeof(local);
  if (getsockname(fd, (struct sockaddr*)&local, &local_len) < 0) {
    perror("getsockname(nonblocking)");
    close(fd);
    close(probe);
    return -1;
  }
  if (addr_port(&local) == addr_port(&addr)) {
    fprintf(stderr, "client and destination unexpectedly share a port\n");
    close(fd);
    close(probe);
    return -1;
  }
  close(probe);

  errno = 0;
  int connect_result = connect(fd, (struct sockaddr*)&addr, addr_len);
  if (connect_result < 0 && errno != EINPROGRESS) {
    fprintf(stderr, "connect returned %d with unexpected errno %d (%s)\n",
            connect_result, errno, strerror(errno));
    close(fd);
    return -1;
  }

  struct pollfd pfd = {.fd = fd, .events = POLLOUT};
  if (poll(&pfd, 1, 1000) != 1) {
    fprintf(stderr, "poll did not report refused connection\n");
    close(fd);
    return -1;
  }
  if (expect_not_connected(fd, "failed connect (first query)") < 0 ||
      expect_not_connected(fd, "failed connect (second query)") < 0) {
    close(fd);
    return -1;
  }

  close(fd);
  return 0;
}

int main(void) {
  const int families[] = {AF_INET, AF_INET6};
  for (size_t i = 0; i < sizeof(families) / sizeof(families[0]); ++i) {
    if (test_unconnected_states(families[i]) < 0 ||
        test_connected(families[i]) < 0 || test_failed_connect(families[i]) < 0)
      return 1;
  }

  puts("getpeername reports actual TCP connection state");
  return 0;
}
