//#ExpectedStdout: boolean socket option used pointed-to value

#include <errno.h>
#include <netinet/in.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  int fd = socket(AF_INET6, SOCK_STREAM, 0);
  if (fd < 0) {
    perror("socket");
    return 1;
  }

  int off = 0;
  if (setsockopt(fd, IPPROTO_IPV6, IPV6_V6ONLY, &off, sizeof(off)) < 0) {
    perror("setsockopt(IPV6_V6ONLY=0)");
    return 1;
  }

  int value = -1;
  socklen_t value_len = sizeof(value);
  if (getsockopt(fd, IPPROTO_IPV6, IPV6_V6ONLY, &value, &value_len) < 0) {
    perror("getsockopt(IPV6_V6ONLY)");
    return 1;
  }

  if (value != 0) {
    fprintf(stderr, "expected IPV6_V6ONLY=0, got %d\n", value);
    return 1;
  }

  puts("boolean socket option used pointed-to value");
  close(fd);
  return 0;
}