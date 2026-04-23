/*
 * Verify that a failed bind(2) leaves the socket logically unbound.
 *
 * Steps:
 *   1. Bind socket A to 127.0.0.1:0 so the OS assigns an ephemeral port.
 *   2. Read the assigned port back with getsockname.
 *   3. Try to bind socket B to the same address/port — this must fail with
 *      EADDRINUSE because socket A still holds the port.
 *   4. Call getsockname on socket B.  The returned port must be 0, proving
 *      the failed bind did not leave a stale address on the socket.
 */
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  /* --- socket A: claim an ephemeral port --- */
  int fd_a = socket(AF_INET, SOCK_STREAM, 0);
  if (fd_a < 0) {
    perror("socket A");
    return 1;
  }

  struct sockaddr_in addr_zero;
  memset(&addr_zero, 0, sizeof(addr_zero));
  addr_zero.sin_family = AF_INET;
  addr_zero.sin_port = htons(0);
  if (inet_pton(AF_INET, "127.0.0.1", &addr_zero.sin_addr) != 1) {
    fprintf(stderr, "inet_pton failed\n");
    close(fd_a);
    return 1;
  }

  if (bind(fd_a, (struct sockaddr*)&addr_zero, sizeof(addr_zero)) < 0) {
    perror("bind A");
    close(fd_a);
    return 1;
  }

  /* Find out which port was assigned to socket A. */
  struct sockaddr_in addr_a;
  socklen_t len = sizeof(addr_a);
  memset(&addr_a, 0, sizeof(addr_a));
  if (getsockname(fd_a, (struct sockaddr*)&addr_a, &len) < 0) {
    perror("getsockname A");
    close(fd_a);
    return 1;
  }

  int port_a = (int)ntohs(addr_a.sin_port);
  if (port_a == 0) {
    fprintf(stderr, "getsockname returned port 0 for socket A\n");
    close(fd_a);
    return 1;
  }

  /* --- socket B: attempt a conflicting bind --- */
  int fd_b = socket(AF_INET, SOCK_STREAM, 0);
  if (fd_b < 0) {
    perror("socket B");
    close(fd_a);
    return 1;
  }

  /* Bind B to the exact same address that A already owns. */
  if (bind(fd_b, (struct sockaddr*)&addr_a, sizeof(addr_a)) == 0) {
    fprintf(stderr, "bind B unexpectedly succeeded on port %d\n", port_a);
    close(fd_a);
    close(fd_b);
    return 1;
  }
  if (errno != EADDRINUSE) {
    fprintf(stderr, "bind B failed with errno %d (%s), expected EADDRINUSE\n",
            errno, strerror(errno));
    close(fd_a);
    close(fd_b);
    return 1;
  }

  /* --- check that socket B is still unbound --- */
  struct sockaddr_in local_b;
  socklen_t len_b = sizeof(local_b);
  memset(&local_b, 0, sizeof(local_b));
  if (getsockname(fd_b, (struct sockaddr*)&local_b, &len_b) < 0) {
    perror("getsockname B");
    close(fd_a);
    close(fd_b);
    return 1;
  }

  int port_b = (int)ntohs(local_b.sin_port);
  if (port_b != 0) {
    fprintf(stderr,
            "after failed bind, getsockname returned port %d for socket B "
            "(expected 0 — socket should still be unbound)\n",
            port_b);
    close(fd_a);
    close(fd_b);
    return 1;
  }

  close(fd_a);
  close(fd_b);
  printf("bind failure leaves socket unbound\n");
  return 0;
}
