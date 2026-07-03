//#ExpectedStdout: getaddrinfo AF_INET returns only IPv4 addresses
//#MinimalLibc: v2026-06-03.1

/*
 * Regression test for wasix-libc getaddrinfo() family filtering.
 *
 * When AF_INET is requested, libc must not return IPv6 results first (or at
 * all). Python's socket.connect() on an AF_INET socket fails with:
 *   TypeError: AF_INET address must be a pair (host, port)
 * if the first getaddrinfo() result is an IPv6 sockaddr.
 */
#include <netdb.h>
#include <netinet/in.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int check_results(const char* host, struct addrinfo* res) {
  if (!res) {
    fprintf(stderr, "getaddrinfo(%s) returned no results\n", host);
    return 1;
  }

  if (res->ai_family != AF_INET) {
    fprintf(
        stderr,
        "getaddrinfo(%s) first result has family %d, expected AF_INET (%d)\n",
        host, res->ai_family, AF_INET);
    return 1;
  }

  if (res->ai_addrlen != sizeof(struct sockaddr_in)) {
    fprintf(stderr,
            "getaddrinfo(%s) first result has addrlen %u, expected %zu\n", host,
            (unsigned)res->ai_addrlen, sizeof(struct sockaddr_in));
    return 1;
  }

  for (struct addrinfo* ai = res; ai; ai = ai->ai_next) {
    if (ai->ai_family != AF_INET) {
      fprintf(stderr, "getaddrinfo(%s) returned family %d in result chain\n",
              host, ai->ai_family);
      return 1;
    }
    if (ai->ai_addrlen != sizeof(struct sockaddr_in)) {
      fprintf(stderr, "getaddrinfo(%s) returned addrlen %u in result chain\n",
              host, (unsigned)ai->ai_addrlen);
      return 1;
    }
  }

  return 0;
}

static int connect_first_af_inet(struct addrinfo* res) {
  int fd = socket(AF_INET, SOCK_DGRAM, 0);
  if (fd < 0) {
    perror("socket");
    return 1;
  }

  if (connect(fd, res->ai_addr, res->ai_addrlen) < 0) {
    perror("connect");
    close(fd);
    return 1;
  }

  close(fd);
  return 0;
}

int main(void) {
  struct addrinfo hints;
  struct addrinfo* res = NULL;
  int err;
  const char* hosts[] = {"localhost", "127.0.0.1", NULL};

  memset(&hints, 0, sizeof(hints));
  hints.ai_family = AF_INET;
  hints.ai_socktype = SOCK_DGRAM;

  for (const char** host = hosts; *host; host++) {
    err = getaddrinfo(*host, "65535", &hints, &res);
    if (err != 0) {
      fprintf(stderr, "getaddrinfo(%s) failed: %s\n", *host, gai_strerror(err));
      return 1;
    }

    if (check_results(*host, res) != 0) {
      freeaddrinfo(res);
      return 1;
    }

    if (connect_first_af_inet(res) != 0) {
      fprintf(stderr, "connect using first getaddrinfo(%s) result failed\n",
              *host);
      freeaddrinfo(res);
      return 1;
    }

    freeaddrinfo(res);
    res = NULL;
  }

  puts("getaddrinfo AF_INET returns only IPv4 addresses");
  return 0;
}
