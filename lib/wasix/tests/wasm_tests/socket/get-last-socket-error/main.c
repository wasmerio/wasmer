//#MinimalLibc: v2026-06-18.1

#include <arpa/inet.h>
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  int fd = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
  if (fd < 0) {
    perror("socket");
    return 1;
  }

  struct sockaddr_in addr = {0};
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr.sin_port = htons(9); /* normally closed discard port */

  errno = 0;
  int connect_res = connect(fd, (struct sockaddr*)&addr, sizeof(addr));
  /* Even if the host rejects the connection immediately, a nonblocking
     connect reports EINPROGRESS and leaves the failure for SO_ERROR. */
  if (connect_res != -1 || errno != EINPROGRESS) {
    fprintf(stderr,
            "connect returned %d with errno %d (%s), expected -1 with "
            "EINPROGRESS\n",
            connect_res, errno, strerror(errno));
    close(fd);
    return 1;
  }

  struct pollfd pfd = {.fd = fd, .events = POLLOUT};
  int poll_res = poll(&pfd, 1, 1000);
  if (poll_res != 1) {
    if (poll_res < 0) {
      perror("poll");
    } else {
      fprintf(stderr, "poll timed out waiting for the connect result\n");
    }
    close(fd);
    return 1;
  }

  int err = 0;
  socklen_t errlen = sizeof(err);
  int res = getsockopt(fd, SOL_SOCKET, SO_ERROR, &err, &errlen);

  if (res < 0) {
    perror("getsockopt(SO_ERROR)");
    close(fd);
    return 1;
  }
  if (err != ECONNREFUSED) {
    fprintf(stderr, "SO_ERROR returned %d (%s), expected ECONNREFUSED\n", err,
            strerror(err));
    close(fd);
    return 1;
  }

  int err2 = 0;
  socklen_t errlen2 = sizeof(err);
  res = getsockopt(fd, SOL_SOCKET, SO_ERROR, &err2, &errlen2);

  if (res < 0) {
    perror("getsockopt(SO_ERROR) after clearing");
    close(fd);
    return 1;
  }
  if (err2 != 0) {
    fprintf(stderr, "second SO_ERROR returned %d (%s), expected success\n",
            err2, strerror(err2));
    close(fd);
    return 1;
  }

  close(fd);
  return 0;
}
