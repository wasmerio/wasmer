#include <arpa/inet.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int get_local_addr(int fd, struct sockaddr_in* addr) {
  socklen_t len = sizeof(*addr);
  memset(addr, 0, sizeof(*addr));
  return getsockname(fd, (struct sockaddr*)addr, &len);
}

int main(void) {
  int fd = socket(AF_INET, SOCK_STREAM, 0);
  if (fd < 0) {
    perror("socket");
    return 1;
  }

  struct sockaddr_in bind_addr;
  memset(&bind_addr, 0, sizeof(bind_addr));
  bind_addr.sin_family = AF_INET;
  bind_addr.sin_port = htons(0);
  if (inet_pton(AF_INET, "127.0.0.1", &bind_addr.sin_addr) != 1) {
    fprintf(stderr, "inet_pton failed\n");
    close(fd);
    return 1;
  }

  if (bind(fd, (struct sockaddr*)&bind_addr, sizeof(bind_addr)) < 0) {
    perror("bind");
    close(fd);
    return 1;
  }

  struct sockaddr_in after_bind;
  if (get_local_addr(fd, &after_bind) < 0) {
    perror("getsockname(after bind)");
    close(fd);
    return 1;
  }

  int bind_port = ntohs(after_bind.sin_port);
  if (bind_port == 0) {
    fprintf(stderr, "expected nonzero ephemeral port after bind, got 0\n");
    close(fd);
    return 1;
  }

  if (listen(fd, 1) < 0) {
    perror("listen");
    close(fd);
    return 1;
  }

  struct sockaddr_in after_listen;
  if (get_local_addr(fd, &after_listen) < 0) {
    perror("getsockname(after listen)");
    close(fd);
    return 1;
  }

  int listen_port = ntohs(after_listen.sin_port);
  if (listen_port != bind_port) {
    fprintf(stderr,
            "expected port to stay stable after listen, got %d then %d\n",
            bind_port, listen_port);
    close(fd);
    return 1;
  }

  puts("bind port 0 allocates an ephemeral port");
  close(fd);
  return 0;
}
