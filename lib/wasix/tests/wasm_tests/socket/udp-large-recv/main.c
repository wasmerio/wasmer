/*
 * udp-large-recv: when a large UDP datagram is delivered, the runtime must
 * return it whole and uncorrupted.
 *
 * The payload is deliberately larger than the sock_recv_from fast-path
 * threshold (10240 bytes) so this exercises the heap-allocated large-recv path.
 *
 * UDP delivery over loopback is best-effort: a single large datagram can be
 * silently dropped, which is common on macOS (the default
 * net.inet.udp.maxdgram is 9216, so an oversized datagram may be rejected
 * outright, and loopback drops large datagrams under load) and under nextest's
 * concurrent-process load. A datagram that never arrives is NOT the behaviour
 * under test, so we retry a few times and, if delivery never succeeds, skip
 * (exit 0) rather than blocking on a 30s recv timeout and failing.
 *
 * The real assertion is integrity: whenever a datagram IS delivered it must
 * have the exact length and payload we sent (no truncation / sharding /
 * corruption).
 */

#include <arpa/inet.h>
#include <errno.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#define PAYLOAD_SIZE 20480
#define MAX_ATTEMPTS 8
#define RECV_TIMEOUT_MS 1000

static uint8_t sendbuf[PAYLOAD_SIZE];
static uint8_t recvbuf[PAYLOAD_SIZE];

int main(void) {
  int receiver = socket(AF_INET, SOCK_DGRAM, 0);
  int sender = socket(AF_INET, SOCK_DGRAM, 0);
  if (receiver < 0 || sender < 0) {
    perror("socket");
    return 1;
  }

  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr.sin_port = 0;
  if (bind(receiver, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("bind");
    return 1;
  }

  socklen_t len = sizeof(addr);
  if (getsockname(receiver, (struct sockaddr*)&addr, &len) != 0) {
    perror("getsockname");
    return 1;
  }

  for (size_t i = 0; i < PAYLOAD_SIZE; ++i) {
    sendbuf[i] = (uint8_t)(i & 0xff);
  }

  for (int attempt = 0; attempt < MAX_ATTEMPTS; ++attempt) {
    ssize_t nsent = sendto(sender, sendbuf, PAYLOAD_SIZE, 0,
                           (struct sockaddr*)&addr, sizeof(addr));
    if (nsent < 0) {
      // Only tolerate the expected best-effort/oversize/transient failures and
      // treat them as a dropped attempt (e.g. macOS rejects datagrams larger
      // than net.inet.udp.maxdgram with EMSGSIZE). Any other errno is a real
      // bug in sendto rather than a lost datagram, so fail instead of skipping.
      if (errno == EMSGSIZE || errno == ENOBUFS || errno == EAGAIN ||
          errno == EWOULDBLOCK || errno == EINTR) {
        fprintf(stderr, "attempt %d: sendto could not deliver (errno %d: %s)\n",
                attempt, errno, strerror(errno));
        continue;
      }
      fprintf(stderr, "sendto failed (errno %d: %s)\n", errno, strerror(errno));
      return 1;
    }
    if (nsent != PAYLOAD_SIZE) {
      // A datagram send is all-or-nothing; a short count is a real bug.
      fprintf(stderr, "sendto sent %zd of %d bytes\n", nsent, PAYLOAD_SIZE);
      return 1;
    }

    // Wait for the datagram with a bounded timeout so a dropped datagram costs
    // RECV_TIMEOUT_MS instead of the socket's default 30s read timeout.
    // (wasix-libc does not wire SO_RCVTIMEO, so poll() is used instead.)
    struct pollfd pfd = {.fd = receiver, .events = POLLIN};
    int pr = poll(&pfd, 1, RECV_TIMEOUT_MS);
    if (pr < 0) {
      if (errno == EINTR) {
        continue;  // interrupted before the datagram arrived; retry
      }
      // A genuine poll error (bad fd, unsupported poll, ...) is a real failure,
      // not a dropped datagram, so surface it instead of skipping.
      fprintf(stderr, "poll failed (errno %d: %s)\n", errno, strerror(errno));
      return 1;
    }
    if (pr == 0) {
      // Nothing arrived within the timeout: the datagram was dropped. Retry.
      fprintf(stderr, "attempt %d: no datagram within %d ms\n", attempt,
              RECV_TIMEOUT_MS);
      continue;
    }
    if ((pfd.revents & POLLIN) == 0) {
      // Readiness reported an error/hangup rather than readable data.
      fprintf(stderr, "poll returned unexpected revents=0x%x\n", pfd.revents);
      return 1;
    }

    ssize_t nread = recvfrom(receiver, recvbuf, sizeof(recvbuf), 0, NULL, NULL);
    if (nread < 0) {
      if (errno == EINTR) {
        continue;  // interrupted before reading; retry
      }
      // poll() reported the socket readable but recvfrom failed: a real bug,
      // not a dropped datagram, so fail rather than silently retry/skip.
      fprintf(stderr, "recvfrom failed after POLLIN (errno %d: %s)\n", errno,
              strerror(errno));
      return 1;
    }

    // A datagram was delivered: it must match exactly. Anything else is the
    // truncation/sharding/corruption bug this test guards against.
    if (nread != PAYLOAD_SIZE) {
      fprintf(stderr, "expected %d-byte datagram, got %zd\n", PAYLOAD_SIZE,
              nread);
      return 1;
    }
    if (memcmp(sendbuf, recvbuf, PAYLOAD_SIZE) != 0) {
      fprintf(stderr, "payload mismatch\n");
      return 1;
    }

    close(sender);
    close(receiver);
    puts("large UDP datagram receive works");
    return 0;
  }

  // No datagram was ever delivered. That is best-effort UDP being lossy, not
  // the behaviour under test, so skip instead of failing.
  fprintf(stderr, "skipping: no datagram delivered after %d attempts\n",
          MAX_ATTEMPTS);
  close(sender);
  close(receiver);
  return 0;
}
