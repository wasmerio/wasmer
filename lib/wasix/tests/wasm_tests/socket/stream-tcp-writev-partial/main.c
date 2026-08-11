//#ExpectedStdout: stream TCP writev crosses the host boundary once
/*
 * Regression test for stream-socket fd_write iovec ordering.
 *
 * VirtualConnectedSocket exposes one contiguous send operation. Implementing
 * writev(2) as a loop of per-iovec send() calls added an artificial scheduling
 * boundary between adjacent buffers: a peer could react to the first iovec
 * before the second crossed the host boundary.
 *
 * Approach:
 *   1. Connect a loopback TCP client and server.
 *   2. Accept the connection and immediately close the server socket.
 *   3. Client writev() with two small iovecs. The virtual socket accepts the
 *      coalesced payload in one send, so both iovecs cross together.
 *
 * Why this is used instead of SO_SNDBUF/window filling:
 *   wasm_tests talk to host TCP. Virtual SO_SNDBUF/SO_RCVBUF tuning is ignored
 *   for host sockets. Closing the peer after accept makes the old per-iovec
 *   implementation reliably return after only the first iovec, while the
 *   single-send implementation accepts the complete small payload.
 */

#include <arpa/inet.h>
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/uio.h>
#include <unistd.h>

enum { FIRST_IOV_LEN = 5, SECOND_IOV_LEN = 5 };

static int accept_one(int listener, struct sockaddr_in* peer) {
  socklen_t len = sizeof(*peer);
  memset(peer, 0, sizeof(*peer));
  return accept(listener, (struct sockaddr*)peer, &len);
}

static int close_peer(int server) { return close(server); }

int main(void) {
  signal(SIGPIPE, SIG_IGN);

  int listener = socket(AF_INET, SOCK_STREAM, 0);
  if (listener < 0) {
    perror("socket(listener)");
    return 1;
  }

  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_port = htons(0);
  if (inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr) != 1) {
    fprintf(stderr, "inet_pton failed\n");
    close(listener);
    return 1;
  }

  if (bind(listener, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("bind(listener)");
    close(listener);
    return 1;
  }

  if (listen(listener, 1) != 0) {
    perror("listen(listener)");
    close(listener);
    return 1;
  }

  socklen_t len = sizeof(addr);
  if (getsockname(listener, (struct sockaddr*)&addr, &len) != 0) {
    perror("getsockname(listener)");
    close(listener);
    return 1;
  }

  int client = socket(AF_INET, SOCK_STREAM, 0);
  if (client < 0) {
    perror("socket(client)");
    close(listener);
    return 1;
  }

  if (connect(client, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("connect(client)");
    close(client);
    close(listener);
    return 1;
  }

  struct sockaddr_in peer;
  int server = accept_one(listener, &peer);
  if (server < 0) {
    perror("accept(server)");
    close(client);
    close(listener);
    return 1;
  }
  close(listener);

  if (close_peer(server) != 0) {
    perror("close_peer(server)");
    close(client);
    return 1;
  }

  struct iovec iov[2] = {
      {.iov_base = "hello", .iov_len = FIRST_IOV_LEN},
      {.iov_base = "world", .iov_len = SECOND_IOV_LEN},
  };
  ssize_t written = writev(client, iov, 2);
  if (written != (ssize_t)(FIRST_IOV_LEN + SECOND_IOV_LEN)) {
    fprintf(stderr,
            "expected writev to return %d bytes in one host send, got %zd "
            "errno=%d (%s)\n",
            FIRST_IOV_LEN + SECOND_IOV_LEN, written, errno, strerror(errno));
    close(client);
    return 1;
  }

  close(client);
  puts("stream TCP writev crosses the host boundary once");
  return 0;
}
