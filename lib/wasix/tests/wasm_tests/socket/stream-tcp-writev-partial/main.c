//#ExpectedStdout: stream TCP writev returns partial success after peer close
/*
 * Regression test for stream-socket fd_write partial success.
 *
 * WASIX implements stream writev(2) as a loop of per-iovec send() calls, not a
 * single atomic kernel writev. When a later send() fails after earlier iovecs
 * succeeded, fd_write must return the bytes already transferred (POSIX writev
 * semantics) instead of failing the whole syscall.
 *
 * Approach:
 *   1. Connect a loopback TCP client and server.
 *   2. Accept the connection and immediately close the server socket.
 *   3. Client writev() with two 5-byte iovecs ("hello", "world").
 *   4. Expect writev to return 5: the first iovec send succeeds, the second
 *      fails with EPIPE/ECONNRESET because the peer is gone.
 *
 * This relies on timing/stack behaviour rather than a tightly controlled setup
 * (e.g. nonblocking socket + small SO_SNDBUF). The first post-close send may
 * still succeed briefly before the stack reports the dead peer on the next
 * send.
 *
 * Flakiness potential:
 *   - If the peer is already fully dead before the first send, writev may
 *     return -1 even when fd_write is correct (false negative).
 *   - If both iovecs succeed before the error is surfaced, writev may return
 *     10 (false negative).
 *   - Behaviour may differ across host TCP, virtual sockets, and CI runners.
 *
 * A more deterministic alternative would be a nonblocking connected socket with
 * a small send buffer, filling the buffer until only the first iovec fits and
 * the second returns EAGAIN with bytes already sent.
 */

#include <arpa/inet.h>
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/uio.h>
#include <unistd.h>

static int accept_one(int listener, struct sockaddr_in* peer) {
  socklen_t len = sizeof(*peer);
  memset(peer, 0, sizeof(*peer));
  return accept(listener, (struct sockaddr*)peer, &len);
}

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

  /*
   * See file header: we depend on the first iovec send succeeding and the
   * second failing after this close, not on delivering exactly half on the wire.
   */
  close(server);
  close(listener);

  struct iovec iov[2] = {
      {.iov_base = "hello", .iov_len = 5},
      {.iov_base = "world", .iov_len = 5},
  };
  ssize_t written = writev(client, iov, 2);
  if (written != 5) {
    fprintf(stderr,
            "expected writev to return 5 bytes after peer close, got %zd "
            "errno=%d (%s)\n",
            written, errno, strerror(errno));
    close(client);
    return 1;
  }

  close(client);
  puts("stream TCP writev returns partial success after peer close");
  return 0;
}
