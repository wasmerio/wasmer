//#MinimalLibc: v2026-06-18.1

#include <arpa/inet.h>
#include <errno.h>
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
  addr.sin_port = htons(9);
  addr.sin_addr.s_addr = htonl(INADDR_BROADCAST);

  errno = 0;
  int connect_res = connect(fd, (struct sockaddr*)&addr, sizeof(addr));
  if (connect_res != -1 || (errno != ENETUNREACH && errno != EIO)) {
    fprintf(stderr,
            "connect returned %d with errno %d (%s), expected -1 with "
            "ENETUNREACH or EIO\n",
            connect_res, errno, strerror(errno));
    close(fd);
    return 1;
  }

  close(fd);
  return 0;
}
