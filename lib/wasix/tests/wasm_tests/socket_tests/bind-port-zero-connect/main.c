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
  int server_fd = socket(AF_INET, SOCK_STREAM, 0);
  if (server_fd < 0) {
    perror("socket(server)");
    return 1;
  }

  struct sockaddr_in server_addr;
  memset(&server_addr, 0, sizeof(server_addr));
  server_addr.sin_family = AF_INET;
  server_addr.sin_port = htons(0);
  if (inet_pton(AF_INET, "127.0.0.1", &server_addr.sin_addr) != 1) {
    fprintf(stderr, "inet_pton(server) failed\n");
    close(server_fd);
    return 1;
  }

  if (bind(server_fd, (struct sockaddr*)&server_addr, sizeof(server_addr)) < 0) {
    perror("bind(server)");
    close(server_fd);
    return 1;
  }

  if (listen(server_fd, 1) < 0) {
    perror("listen(server)");
    close(server_fd);
    return 1;
  }

  struct sockaddr_in server_bound_addr;
  if (get_local_addr(server_fd, &server_bound_addr) < 0) {
    perror("getsockname(server)");
    close(server_fd);
    return 1;
  }

  int client_fd = socket(AF_INET, SOCK_STREAM, 0);
  if (client_fd < 0) {
    perror("socket(client)");
    close(server_fd);
    return 1;
  }

  struct sockaddr_in client_bind_addr;
  memset(&client_bind_addr, 0, sizeof(client_bind_addr));
  client_bind_addr.sin_family = AF_INET;
  client_bind_addr.sin_port = htons(0);
  if (inet_pton(AF_INET, "127.0.0.1", &client_bind_addr.sin_addr) != 1) {
    fprintf(stderr, "inet_pton(client) failed\n");
    close(client_fd);
    close(server_fd);
    return 1;
  }

  if (bind(client_fd, (struct sockaddr*)&client_bind_addr, sizeof(client_bind_addr)) < 0) {
    perror("bind(client)");
    close(client_fd);
    close(server_fd);
    return 1;
  }

  struct sockaddr_in client_after_bind;
  if (get_local_addr(client_fd, &client_after_bind) < 0) {
    perror("getsockname(client after bind)");
    close(client_fd);
    close(server_fd);
    return 1;
  }

  int bind_port = ntohs(client_after_bind.sin_port);
  if (bind_port == 0) {
    fprintf(stderr, "expected nonzero client port after bind, got 0\n");
    close(client_fd);
    close(server_fd);
    return 1;
  }

  if (connect(client_fd, (struct sockaddr*)&server_bound_addr, sizeof(server_bound_addr)) < 0) {
    perror("connect(client)");
    close(client_fd);
    close(server_fd);
    return 1;
  }

  struct sockaddr_in client_after_connect;
  if (get_local_addr(client_fd, &client_after_connect) < 0) {
    perror("getsockname(client after connect)");
    close(client_fd);
    close(server_fd);
    return 1;
  }

  int connect_port = ntohs(client_after_connect.sin_port);
  if (connect_port != bind_port) {
    fprintf(stderr,
            "expected client port to stay stable across connect, got %d then %d\n",
            bind_port, connect_port);
    close(client_fd);
    close(server_fd);
    return 1;
  }

  puts("bind port 0 keeps the same ephemeral port across connect");
  close(client_fd);
  close(server_fd);
  return 0;
}
