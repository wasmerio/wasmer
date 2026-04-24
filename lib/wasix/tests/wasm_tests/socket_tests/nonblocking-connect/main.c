#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>

static long elapsed_ms(struct timeval start, struct timeval end) {
  long seconds = end.tv_sec - start.tv_sec;
  long useconds = end.tv_usec - start.tv_usec;
  return seconds * 1000 + useconds / 1000;
}

int main(void) {
  int fd = socket(AF_INET, SOCK_STREAM, 0);
  if (fd < 0) {
    perror("socket");
    return 1;
  }

  int flags = fcntl(fd, F_GETFL, 0);
  if (flags < 0) {
    perror("fcntl(F_GETFL)");
    close(fd);
    return 1;
  }

  if (fcntl(fd, F_SETFL, flags | O_NONBLOCK) < 0) {
    perror("fcntl(F_SETFL)");
    close(fd);
    return 1;
  }

  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_port = htons(443);
  if (inet_pton(AF_INET, "10.255.255.1", &addr.sin_addr) != 1) {
    fprintf(stderr, "inet_pton failed\n");
    close(fd);
    return 1;
  }

  struct timeval start;
  struct timeval end;
  gettimeofday(&start, NULL);
  int rc = connect(fd, (struct sockaddr*)&addr, sizeof(addr));
  gettimeofday(&end, NULL);

  long ms = elapsed_ms(start, end);
  close(fd);

  if (ms > 1000) {
    fprintf(stderr,
            "nonblocking connect took too long: %ldms (expected immediate "
            "return)\n",
            ms);
    return 1;
  }

  if (rc == 0) {
    fprintf(
        stderr,
        "unexpected immediate success for blackholed nonblocking connect\n");
    return 1;
  }

  switch (errno) {
    case EINPROGRESS:
    case EALREADY:
    case EWOULDBLOCK:
    case ENETUNREACH:
    case EHOSTUNREACH:
    case ECONNREFUSED:
      printf("nonblocking connect returned immediately\n");
      return 0;
    default:
      fprintf(stderr, "unexpected connect result: rc=%d errno=%d (%s)\n", rc,
              errno, strerror(errno));
      return 1;
  }
}
