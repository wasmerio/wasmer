//#ExpectedStdout: SO_ERROR: Success

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
  close(fd);

  if (!res) {
    fprintf(stderr, "Cannot get socket error\n");
    return 1;
  }

  printf("SO_ERROR: %s\n", strerror(err));
  return 0;
}