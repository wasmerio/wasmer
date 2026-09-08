//#AbstractConfig: false-bool
//#MinimalLibc: v2026-06-18.1
//
//#Config: explicit-v6only
//#Args: explicit-v6only
//#ExpectedStdout: explicit IPV6_V6ONLY preserved
//
//#Config: explicit-dual-stack:false-bool
//#Args: explicit-dual-stack
//#ExpectedStdout: explicit dual-stack setting preserved
//
//#Config: replacement:false-bool
//#Args: replacement
//#ExpectedStdout: latest IPV6_V6ONLY setting preserved
//
//#Config: autobind-connect
//#Args: autobind-connect
//#ExpectedStdout: connect autobind preserved IPV6_V6ONLY
//
//#Config: autobind-sendto
//#Args: autobind-sendto
//#ExpectedStdout: sendto autobind preserved IPV6_V6ONLY

#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int set_only_v6(int fd, int value) {
  if (setsockopt(fd, IPPROTO_IPV6, IPV6_V6ONLY, &value, sizeof(value)) < 0) {
    perror("setsockopt(IPV6_V6ONLY)");
    return -1;
  }
  return 0;
}

static int expect_only_v6(int fd, int expected) {
  int value = -1;
  socklen_t len = sizeof(value);
  if (getsockopt(fd, IPPROTO_IPV6, IPV6_V6ONLY, &value, &len) < 0) {
    perror("getsockopt(IPV6_V6ONLY)");
    return -1;
  }
  if (len != sizeof(value) || value != expected) {
    fprintf(stderr, "IPV6_V6ONLY: expected %d, got %d (length %u)\n", expected,
            value, (unsigned)len);
    return -1;
  }
  return 0;
}

static int bind_ipv6_any(int fd) {
  struct sockaddr_in6 addr = {
      .sin6_family = AF_INET6,
      .sin6_port = htons(0),
      .sin6_addr = IN6ADDR_ANY_INIT,
  };
  if (bind(fd, (const struct sockaddr*)&addr, sizeof(addr)) < 0) {
    perror("bind(AF_INET6)");
    return -1;
  }
  return 0;
}

static int local_port(int fd) {
  struct sockaddr_in6 addr;
  socklen_t len = sizeof(addr);
  if (getsockname(fd, (struct sockaddr*)&addr, &len) < 0) {
    perror("getsockname");
    return -1;
  }
  if (len != sizeof(addr) || addr.sin6_family != AF_INET6 ||
      addr.sin6_port == 0) {
    fputs("unexpected IPv6 local address\n", stderr);
    return -1;
  }
  return ntohs(addr.sin6_port);
}

static int bind_ipv4_port(int port) {
  int fd = socket(AF_INET, SOCK_DGRAM, 0);
  if (fd < 0) {
    perror("socket(AF_INET)");
    return -1;
  }
  struct sockaddr_in addr = {
      .sin_family = AF_INET,
      .sin_port = htons((uint16_t)port),
      .sin_addr.s_addr = htonl(INADDR_ANY),
  };
  if (bind(fd, (const struct sockaddr*)&addr, sizeof(addr)) < 0) {
    int saved_errno = errno;
    close(fd);
    errno = saved_errno;
    return -1;
  }
  return fd;
}

static int new_ipv6_socket(void) {
  int fd = socket(AF_INET6, SOCK_DGRAM, 0);
  if (fd < 0) perror("socket(AF_INET6)");
  return fd;
}

static int test_explicit_v6only(void) {
  int fd6 = new_ipv6_socket();
  if (fd6 < 0 || set_only_v6(fd6, 1) < 0 || bind_ipv6_any(fd6) < 0 ||
      expect_only_v6(fd6, 1) < 0)
    return EXIT_FAILURE;

  int port = local_port(fd6);
  int fd4 = port < 0 ? -1 : bind_ipv4_port(port);
  if (fd4 < 0) {
    perror("IPv4 co-bind with IPV6_V6ONLY=1");
    return EXIT_FAILURE;
  }

  close(fd4);
  close(fd6);
  puts("explicit IPV6_V6ONLY preserved");
  return EXIT_SUCCESS;
}

static int test_dual_stack(int replacement) {
  int fd6 = new_ipv6_socket();
  if (fd6 < 0 || (replacement && set_only_v6(fd6, 1) < 0) ||
      set_only_v6(fd6, 0) < 0 || expect_only_v6(fd6, 0) < 0 ||
      bind_ipv6_any(fd6) < 0 || expect_only_v6(fd6, 0) < 0)
    return EXIT_FAILURE;

  int port = local_port(fd6);
  errno = 0;
  int fd4 = port < 0 ? -1 : bind_ipv4_port(port);
  if (fd4 >= 0 || errno != EADDRINUSE) {
    if (fd4 >= 0) close(fd4);
    fprintf(stderr,
            "IPv4 co-bind with IPV6_V6ONLY=0: expected EADDRINUSE, got %s\n",
            strerror(errno));
    return EXIT_FAILURE;
  }

  close(fd6);
  puts(replacement ? "latest IPV6_V6ONLY setting preserved"
                   : "explicit dual-stack setting preserved");
  return EXIT_SUCCESS;
}

static int test_autobind(int use_connect) {
  int fd = new_ipv6_socket();
  if (fd < 0 || set_only_v6(fd, 1) < 0) return EXIT_FAILURE;

  struct sockaddr_in6 peer = {
      .sin6_family = AF_INET6,
      .sin6_port = htons(9),
      .sin6_addr = IN6ADDR_LOOPBACK_INIT,
  };
  if (use_connect) {
    if (connect(fd, (const struct sockaddr*)&peer, sizeof(peer)) < 0) {
      perror("connect");
      return EXIT_FAILURE;
    }
  } else if (sendto(fd, "x", 1, 0, (const struct sockaddr*)&peer,
                    sizeof(peer)) != 1) {
    perror("sendto");
    return EXIT_FAILURE;
  }

  if (local_port(fd) < 0 || expect_only_v6(fd, 1) < 0) return EXIT_FAILURE;

  close(fd);
  puts(use_connect ? "connect autobind preserved IPV6_V6ONLY"
                   : "sendto autobind preserved IPV6_V6ONLY");
  return EXIT_SUCCESS;
}

int main(int argc, char** argv) {
  if (argc != 2) return EXIT_FAILURE;
  if (!strcmp(argv[1], "explicit-v6only")) return test_explicit_v6only();
  if (!strcmp(argv[1], "explicit-dual-stack")) return test_dual_stack(0);
  if (!strcmp(argv[1], "replacement")) return test_dual_stack(1);
  if (!strcmp(argv[1], "autobind-connect")) return test_autobind(1);
  if (!strcmp(argv[1], "autobind-sendto")) return test_autobind(0);
  return EXIT_FAILURE;
}
