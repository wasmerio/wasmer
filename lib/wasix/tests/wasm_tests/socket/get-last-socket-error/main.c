//#ExpectedStdout: SO_ERROR: Connection refused, Success

#include <arpa/inet.h>
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  int fd = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);

  struct sockaddr_in addr = {0};
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr.sin_port = htons(9); /* normally closed discard port */

  connect(fd, (struct sockaddr*)&addr, sizeof(addr));

  struct pollfd pfd = {.fd = fd, .events = POLLOUT};
  poll(&pfd, 1, 1000);

  int err = 0;
  socklen_t errlen = sizeof(err);
  int res = getsockopt(fd, SOL_SOCKET, SO_ERROR, &err, &errlen);

  if (res) {
    close(fd);
    fprintf(stderr, "Cannot get socket error\n");
    return 1;
  }

  int err2 = 0;
  socklen_t errlen2 = sizeof(err);
  res = getsockopt(fd, SOL_SOCKET, SO_ERROR, &err2, &errlen2);

  if (res) {
    close(fd);
    fprintf(stderr, "Cannot get socket error (2)\n");
    return 1;
  }

  close(fd);
  printf("SO_ERROR: %s, %s\n", strerror(err), strerror(err2));

  return 0;
}
