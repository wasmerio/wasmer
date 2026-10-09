//#Config: first
//#Networking: loopback
//#ExpectedStdout: loopback TCP address matching works
//#Config: second:first

#include <arpa/inet.h>
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static struct sockaddr_in ipv4_addr(const char* ip, unsigned short port) {
  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_port = htons(port);
  if (inet_pton(AF_INET, ip, &addr.sin_addr) != 1) {
    fprintf(stderr, "inet_pton failed for %s\n", ip);
  }
  return addr;
}

static int local_addr(int fd, struct sockaddr_in* addr) {
  socklen_t len = sizeof(*addr);
  memset(addr, 0, sizeof(*addr));
  return getsockname(fd, (struct sockaddr*)addr, &len);
}

static int peer_addr(int fd, struct sockaddr_in* addr) {
  socklen_t len = sizeof(*addr);
  memset(addr, 0, sizeof(*addr));
  return getpeername(fd, (struct sockaddr*)addr, &len);
}

static int bind_client(int fd) {
  struct sockaddr_in addr = ipv4_addr("0.0.0.0", 0);
  return bind(fd, (struct sockaddr*)&addr, sizeof(addr));
}

static int expect_bind_conflict(const char* label, const char* ip,
                                unsigned short port) {
  int fd = socket(AF_INET, SOCK_STREAM, 0);
  struct sockaddr_in addr = ipv4_addr(ip, port);
  if (fd < 0) {
    perror("socket(conflict)");
    return -1;
  }
  errno = 0;
  if (bind(fd, (struct sockaddr*)&addr, sizeof(addr)) == 0 ||
      errno != EADDRINUSE) {
    fprintf(stderr, "%s: expected EADDRINUSE, got errno %d (%s)\n", label,
            errno, strerror(errno));
    close(fd);
    return -1;
  }
  close(fd);
  return 0;
}

static int test_wildcard_listener(void) {
  int listener = socket(AF_INET, SOCK_STREAM, 0);
  struct sockaddr_in wildcard = ipv4_addr("0.0.0.0", 0);
  if (listener < 0 ||
      bind(listener, (struct sockaddr*)&wildcard, sizeof(wildcard)) != 0 ||
      listen(listener, 2) != 0) {
    perror("wildcard listener");
    return -1;
  }

  struct sockaddr_in bound;
  if (local_addr(listener, &bound) != 0) {
    perror("getsockname(wildcard listener)");
    return -1;
  }
  if (bound.sin_addr.s_addr != htonl(INADDR_ANY) ||
      ntohs(bound.sin_port) == 0) {
    fprintf(stderr, "wildcard bind was not preserved\n");
    return -1;
  }
  unsigned short port = ntohs(bound.sin_port);
  if (expect_bind_conflict("wildcard listener then specific bind", "127.0.0.42",
                           port) != 0) {
    return -1;
  }

  struct sockaddr_in destination = ipv4_addr("127.0.0.42", port);
  int client = socket(AF_INET, SOCK_STREAM, 0);
  if (client < 0 || bind_client(client) != 0 ||
      connect(client, (struct sockaddr*)&destination, sizeof(destination)) !=
          0) {
    perror("connect(wildcard listener)");
    return -1;
  }

  struct sockaddr_in client_local;
  struct sockaddr_in client_peer;
  if (local_addr(client, &client_local) != 0 ||
      peer_addr(client, &client_peer) != 0) {
    perror("getname(client)");
    return -1;
  }
  if (client_local.sin_addr.s_addr == htonl(INADDR_ANY) ||
      client_peer.sin_addr.s_addr != destination.sin_addr.s_addr ||
      client_peer.sin_port != destination.sin_port) {
    fprintf(stderr, "client connection endpoints were not concrete\n");
    return -1;
  }

  struct sockaddr_in accepted_peer;
  socklen_t accepted_peer_len = sizeof(accepted_peer);
  int accepted =
      accept(listener, (struct sockaddr*)&accepted_peer, &accepted_peer_len);
  struct sockaddr_in accepted_local;
  if (accepted < 0 || local_addr(accepted, &accepted_local) != 0) {
    perror("accept(wildcard listener)");
    return -1;
  }
  if (accepted_local.sin_addr.s_addr != destination.sin_addr.s_addr ||
      accepted_local.sin_port != destination.sin_port ||
      accepted_peer.sin_addr.s_addr != client_local.sin_addr.s_addr ||
      accepted_peer.sin_port != client_local.sin_port) {
    fprintf(stderr, "accepted connection endpoints were not concrete\n");
    return -1;
  }

  close(accepted);
  close(client);
  close(listener);
  return 0;
}

static int test_specific_listeners(void) {
  int first = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
  // Reusing this port detects a network shared between fixture configurations.
  struct sockaddr_in first_addr = ipv4_addr("127.0.0.1", 40205);
  if (first < 0 ||
      bind(first, (struct sockaddr*)&first_addr, sizeof(first_addr)) != 0 ||
      listen(first, 2) != 0 || local_addr(first, &first_addr) != 0) {
    perror("first specific listener");
    return -1;
  }

  unsigned short port = ntohs(first_addr.sin_port);
  struct sockaddr_in second_addr = ipv4_addr("127.0.0.2", port);
  int second = socket(AF_INET, SOCK_STREAM, 0);
  if (second < 0 ||
      bind(second, (struct sockaddr*)&second_addr, sizeof(second_addr)) != 0 ||
      listen(second, 2) != 0) {
    perror("second specific listener");
    return -1;
  }
  if (expect_bind_conflict("specific listeners then wildcard bind", "0.0.0.0",
                           port) != 0) {
    return -1;
  }

  int client = socket(AF_INET, SOCK_STREAM, 0);
  if (client < 0 || bind_client(client) != 0 ||
      connect(client, (struct sockaddr*)&second_addr, sizeof(second_addr)) !=
          0) {
    perror("connect(second specific listener)");
    return -1;
  }
  int accepted = accept(second, NULL, NULL);
  if (accepted < 0) {
    perror("accept(second specific listener)");
    return -1;
  }
  struct pollfd first_poll = {.fd = first, .events = POLLIN};
  if (poll(&first_poll, 1, 0) != 0 || (first_poll.revents & POLLIN) != 0) {
    fprintf(stderr, "connection was delivered to the wrong exact listener\n");
    return -1;
  }

  struct sockaddr_in missing = ipv4_addr("127.0.0.3", port);
  int missing_client = socket(AF_INET, SOCK_STREAM, 0);
  errno = 0;
  if (missing_client < 0 || bind_client(missing_client) != 0 ||
      connect(missing_client, (struct sockaddr*)&missing, sizeof(missing)) ==
          0 ||
      errno != ECONNREFUSED) {
    fprintf(stderr,
            "missing destination unexpectedly matched a listener: %d (%s)\n",
            errno, strerror(errno));
    return -1;
  }

  close(missing_client);
  close(accepted);
  close(client);
  close(second);
  close(first);
  return 0;
}

int main(void) {
  if (test_wildcard_listener() != 0 || test_specific_listeners() != 0) {
    return 1;
  }
  puts("loopback TCP address matching works");
  return 0;
}
