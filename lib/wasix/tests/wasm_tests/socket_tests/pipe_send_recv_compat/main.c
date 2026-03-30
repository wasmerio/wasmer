#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  int pipefd[2];
  if (pipe(pipefd) != 0) {
    perror("pipe");
    return 1;
  }

  int sent = send(pipefd[1], "ping", 4, 0);
  if (sent != 4) {
    perror("send");
    return 1;
  }

  char buf[5];
  int received = recv(pipefd[0], buf, 4, 0);
  if (received != 4) {
    perror("recv");
    return 1;
  }

  buf[received] = '\0';
  if (strcmp(buf, "ping") != 0) {
    fprintf(stderr, "unexpected payload: %s\n", buf);
    return 1;
  }

  printf("pipe send/recv works\n");
  return 0;
}
