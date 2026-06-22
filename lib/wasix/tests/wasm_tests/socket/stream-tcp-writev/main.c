//#ExpectedStdout: stream TCP writev delivers full payload

#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/uio.h>
#include <unistd.h>

static int accept_one(int listener, struct sockaddr_in* peer) {
  socklen_t len = sizeof(*peer);
  memset(peer, 0, sizeof(*peer));
  return accept(listener, (struct sockaddr*)peer, &len);
}

int main(void) {
  int listener = socket(AF_INET, SOCK_STREAM, 0);
  if (listener < 0) {
    perror("socket(listener)");
    return 1;
  }

  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_port = htons(0);
  if (inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr) != 1) {
    fprintf(stderr, "inet_pton failed\n");
    close(listener);
    return 1;
  }

  if (bind(listener, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("bind(listener)");
    close(listener);
    return 1;
  }

  if (listen(listener, 1) != 0) {
    perror("listen(listener)");
    close(listener);
    return 1;
  }

  socklen_t len = sizeof(addr);
  if (getsockname(listener, (struct sockaddr*)&addr, &len) != 0) {
    perror("getsockname(listener)");
    close(listener);
    return 1;
  }

  int client = socket(AF_INET, SOCK_STREAM, 0);
  if (client < 0) {
    perror("socket(client)");
    close(listener);
    return 1;
  }

  if (connect(client, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("connect(client)");
    close(client);
    close(listener);
    return 1;
  }

  struct sockaddr_in peer;
  int server = accept_one(listener, &peer);
  if (server < 0) {
    perror("accept(server)");
    close(client);
    close(listener);
    return 1;
  }

  struct iovec iov[2] = {
      {.iov_base = "he", .iov_len = 2},
      {.iov_base = "llo", .iov_len = 3},
  };
  ssize_t written = writev(client, iov, 2);
  if (written != 5) {
    fprintf(stderr, "expected writev to write 5 bytes, got %zd errno=%d (%s)\n",
            written, errno, strerror(errno));
    close(client);
    close(server);
    close(listener);
    return 1;
  }

  char buf[16] = {0};
  ssize_t nread = read(server, buf, sizeof(buf));
  if (nread != 5 || memcmp(buf, "hello", 5) != 0) {
    fprintf(stderr, "expected to read `hello`, got %zd bytes: %.*s\n", nread,
            nread > 0 ? (int)nread : 0, buf);
    close(client);
    close(server);
    close(listener);
    return 1;
  }

  close(client);
  close(server);
  close(listener);
  puts("stream TCP writev delivers full payload");
  return 0;
}
