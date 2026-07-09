//#Ignored: driven only by the writev_partial_send_error mock-networking harness test
/*
 * Guest driver for the stream writev partial-success-on-later-error path.
 *
 * This is NOT run against real host networking (hence the Ignored directive
 * above, which makes the auto-collected run skip it). It is compiled and run by
 * the explicit `wasm/writev_partial_send_error` harness test against a mock
 * VirtualNetworking whose TCP socket succeeds on the first send() and returns
 * ECONNRESET on the second. That deterministically drives fd_write's per-iovec
 * loop down the `Err(_) if sent > 0 => break` branch: the first iovec is fully
 * sent, the second send() errors, and writev must return the bytes already
 * transferred (the first iovec length) instead of failing the whole syscall.
 *
 * This complements stream-tcp-writev-partial, which covers the short-write
 * branch of the same contract deterministically but cannot reach the error
 * branch without racing an asynchronous RST (issue #6785).
 */

#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/uio.h>
#include <unistd.h>

enum { FIRST_IOV_LEN = 5, SECOND_IOV_LEN = 5 };

int main(void) {
  int client = socket(AF_INET, SOCK_STREAM, 0);
  if (client < 0) {
    perror("socket");
    return 1;
  }

  // The mock networking ignores the destination, so any valid address works.
  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_port = htons(1234);
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  if (connect(client, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("connect");
    close(client);
    return 1;
  }

  struct iovec iov[2] = {
      {.iov_base = "hello", .iov_len = FIRST_IOV_LEN},
      {.iov_base = "world", .iov_len = SECOND_IOV_LEN},
  };

  // The first per-iovec send() succeeds; the second returns ECONNRESET. writev
  // must report the bytes already transferred, i.e. exactly FIRST_IOV_LEN.
  ssize_t written = writev(client, iov, 2);
  if (written != (ssize_t)FIRST_IOV_LEN) {
    fprintf(stderr,
            "expected writev to return %d after a later send error, got %zd "
            "errno=%d (%s)\n",
            FIRST_IOV_LEN, written, errno, strerror(errno));
    close(client);
    return 1;
  }

  close(client);
  puts("stream TCP writev returns partial count after later send error");
  return 0;
}
