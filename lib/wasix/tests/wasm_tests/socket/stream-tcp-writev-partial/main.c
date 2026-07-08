//#ExpectedStdout: stream TCP writev returns partial success after peer close
//#Ignored: flaky test (#6785)

/*
 * Regression test for stream-socket fd_write partial success.
 *
 * WASIX implements stream writev(2) as a loop of per-iovec send() calls. When
 * a later send() fails after earlier iovecs succeeded, fd_write must return the
 * bytes already transferred (POSIX writev semantics) instead of failing the
 * whole syscall.
 *
 * Approach:
 *   1. Connect a loopback TCP client and server.
 *   2. Accept the connection and immediately close the server socket.
 *   3. Client writev() with two small iovecs. The first per-iovec send() still
 *      succeeds, the second returns EPIPE/EAGAIN, and the syscall must return
 *      only the first iovec length.
 *
 * Why this is used instead of SO_SNDBUF/window filling:
 *   wasm_tests talk to host TCP. Virtual SO_SNDBUF/SO_RCVBUF tuning is ignored
 *   for host sockets, so "fill the send buffer then writev" still completes the
 *   second iovec via partial Ok(...) sends rather than Err(...). Closing the
 *   peer after accept reliably drives the second per-iovec send() down the
 *   error path while the first one has already succeeded, which is exactly the
 *   branch fixed in fd_write.
 *
 * This depends on WASIX's per-iovec stream writev implementation rather than on
 * atomic host-kernel writev behaviour.
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
  if (written != (ssize_t)FIRST_IOV_LEN) {
    fprintf(stderr,
            "expected writev to return %d bytes after peer close, got %zd "
            "errno=%d (%s)\n",
            FIRST_IOV_LEN, written, errno, strerror(errno));
    close(client);
    return 1;
  }

  close(client);
  puts("stream TCP writev returns partial success after peer close");
  return 0;
}
