//#ExpectedStdout: getnameinfo loopback returns localhost
//#MinimalLibc: v2026-06-03.1

/*
 * Regression test for WASIX loopback reverse lookup.
 *
 * getnameinfo() should be able to resolve the canonical loopback addresses to
 * localhost even when /etc/hosts is unavailable. IPv6 scope ids are not
 * meaningful for ::1, and IPv4-mapped 127.0.0.1 should follow the same
 * loopback fallback as plain IPv4.
 */
#include <arpa/inet.h>
#include <netdb.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>

static int expect_host(const struct sockaddr* addr, socklen_t addrlen,
                       const char* label) {
  char host[NI_MAXHOST];
  int err =
      getnameinfo(addr, addrlen, host, sizeof(host), NULL, 0, NI_NAMEREQD);
  if (err != 0) {
    fprintf(stderr, "%s: getnameinfo failed: %s\n", label, gai_strerror(err));
    return 1;
  }

  if (strcmp(host, "localhost") != 0) {
    fprintf(stderr, "%s: expected localhost, got %s\n", label, host);
    return 1;
  }

  return 0;
}

static int expect_ipv4_loopback(void) {
  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_port = htons(80);
  if (inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr) != 1) {
    fprintf(stderr, "inet_pton(AF_INET) failed\n");
    return 1;
  }

  return expect_host((const struct sockaddr*)&addr, sizeof(addr), "127.0.0.1");
}

static int expect_ipv6_loopback_with_scope(void) {
  struct sockaddr_in6 addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin6_family = AF_INET6;
  addr.sin6_port = htons(80);
  addr.sin6_scope_id = 7;
  if (inet_pton(AF_INET6, "::1", &addr.sin6_addr) != 1) {
    fprintf(stderr, "inet_pton(AF_INET6 ::1) failed\n");
    return 1;
  }

  return expect_host((const struct sockaddr*)&addr, sizeof(addr), "::1%7");
}

static int expect_ipv4_mapped_loopback(void) {
  struct sockaddr_in6 addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin6_family = AF_INET6;
  addr.sin6_port = htons(80);
  if (inet_pton(AF_INET6, "::ffff:127.0.0.1", &addr.sin6_addr) != 1) {
    fprintf(stderr, "inet_pton(AF_INET6 ::ffff:127.0.0.1) failed\n");
    return 1;
  }

  return expect_host((const struct sockaddr*)&addr, sizeof(addr),
                     "::ffff:127.0.0.1");
}

int main(void) {
  if (expect_ipv4_loopback() != 0) return 1;
  if (expect_ipv6_loopback_with_scope() != 0) return 1;
  if (expect_ipv4_mapped_loopback() != 0) return 1;

  puts("getnameinfo loopback returns localhost");
  return 0;
}
