/*
 * Verify that a successfully bound TCP socket is immediately reported writable
 * by select(2), matching Linux semantics.
 *
 * On Linux:
 *   int fd = socket(...); bind(fd, ...);
 *   select(fd+1, NULL, &wfds, NULL, &zero_timeout);
 * returns 1 — the socket is writable right away.
 */
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  int fd = socket(AF_INET, SOCK_STREAM, 0);
  if (fd < 0) {
    perror("socket");
    return 1;
  }

  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_port = htons(0);
  if (inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr) != 1) {
    fprintf(stderr, "inet_pton failed\n");
    close(fd);
    return 1;
  }

  if (bind(fd, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
    perror("bind");
    close(fd);
    return 1;
  }

  /* Zero-timeout select: only report what is ready RIGHT NOW. */
  fd_set wfds;
  FD_ZERO(&wfds);
  FD_SET(fd, &wfds);
  struct timeval tv;
  tv.tv_sec = 0;
  tv.tv_usec = 0;

  int n = select(fd + 1, NULL, &wfds, NULL, &tv);
  if (n < 0) {
    perror("select");
    close(fd);
    return 1;
  }

  if (n == 0 || !FD_ISSET(fd, &wfds)) {
    fprintf(stderr,
            "bound TCP socket not reported writable by select "
            "(n=%d) — expected writable immediately after bind\n",
            n);
    close(fd);
    return 1;
  }

  close(fd);
  printf("bound TCP socket is writable\n");
  return 0;
}
