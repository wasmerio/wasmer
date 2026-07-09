//#ExpectedStdout: stream TCP writev returns partial count on short write
/*
 * Regression test for stream-socket fd_write partial success.
 *
 * WASIX implements stream writev(2) as a loop of per-iovec send() calls. When a
 * later iovec cannot be fully transferred after earlier iovecs already
 * succeeded, fd_write must return the number of bytes already transferred
 * (POSIX writev semantics) instead of failing the whole syscall.
 *
 * Approach (deterministic):
 *   1. Connect a loopback TCP client and server, and never read on the server
 *      so the connection's buffers can be driven full.
 *   2. Make the client non-blocking so a full send buffer produces a short
 *      write instead of blocking.
 *   3. writev() two iovecs: a tiny first one and a second one far larger than
 *      any loopback socket buffer. On an empty send buffer the first iovec is
 *      always accepted in full; the oversized second iovec can never fit, so
 *      its send() is a short write. fd_write must break out of the per-iovec
 *      loop and return the partial total (first iovec + whatever of the second
 *      was accepted).
 *
 * Why this shape:
 *   The obvious alternative - close the peer and rely on a later send() failing
 *   with EPIPE/ECONNRESET after an earlier one succeeded - is inherently racy.
 *   That requires the peer's RST to be processed in the window between two
 *   back-to-back internal send() calls of a single writev; depending on RST
 *   timing the syscall returns the first iovec length, the full length, or -1,
 *   which made this test flaky (issue #6785). Virtual SO_SNDBUF/SO_RCVBUF
 *   tuning is a no-op for host sockets, so the buffer cannot be shrunk to make
 *   the boundary controllable either. Forcing a short write with an oversized
 *   iovec exercises the same "return bytes already transferred" contract with
 *   no dependence on asynchronous error timing.
 */

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/uio.h>
#include <unistd.h>

enum { FIRST_IOV_LEN = 5 };

// Larger than any plausible loopback TCP send+receive buffer, so the second
// iovec is guaranteed to be a short write regardless of host buffer autotuning.
#define SECOND_IOV_LEN (64 * 1024 * 1024)

int main(void) {
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

  // Accept but never read, so the send path can be driven to a short write.
  int server = accept(listener, NULL, NULL);
  if (server < 0) {
    perror("accept(server)");
    close(client);
    close(listener);
    return 1;
  }
  close(listener);

  // Non-blocking: a full send buffer yields a short write instead of blocking.
  int flags = fcntl(client, F_GETFL, 0);
  if (flags < 0 || fcntl(client, F_SETFL, flags | O_NONBLOCK) != 0) {
    perror("fcntl(O_NONBLOCK)");
    close(client);
    close(server);
    return 1;
  }

  char* big = malloc(SECOND_IOV_LEN);
  if (big == NULL) {
    fprintf(stderr, "malloc failed\n");
    close(client);
    close(server);
    return 1;
  }
  memset(big, 'w', SECOND_IOV_LEN);

  struct iovec iov[2] = {
      {.iov_base = "hello", .iov_len = FIRST_IOV_LEN},
      {.iov_base = big, .iov_len = SECOND_IOV_LEN},
  };

  ssize_t written = writev(client, iov, 2);

  // The first iovec always fits an empty send buffer, and the oversized second
  // iovec never fully fits, so the result must be a partial total: strictly
  // greater than the first iovec length and strictly less than the full length.
  // A whole-syscall failure (the regression) would surface as -1 here.
  if (written <= (ssize_t)FIRST_IOV_LEN ||
      written >= (ssize_t)(FIRST_IOV_LEN + (size_t)SECOND_IOV_LEN)) {
    fprintf(stderr,
            "expected partial writev in (%d, %zu), got %zd errno=%d (%s)\n",
            FIRST_IOV_LEN, FIRST_IOV_LEN + (size_t)SECOND_IOV_LEN, written,
            errno, strerror(errno));
    free(big);
    close(client);
    close(server);
    return 1;
  }

  free(big);
  close(client);
  close(server);
  puts("stream TCP writev returns partial count on short write");
  return 0;
}
