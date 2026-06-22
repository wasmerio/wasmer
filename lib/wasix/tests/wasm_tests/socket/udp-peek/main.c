//#ExpectedStdout: UDP MSG_PEEK leaves datagram queued

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  int receiver = socket(AF_INET, SOCK_DGRAM, 0);
  int sender = socket(AF_INET, SOCK_DGRAM, 0);
  if (receiver < 0 || sender < 0) {
    perror("socket");
    return 1;
  }

  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr.sin_port = 0;
  if (bind(receiver, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("bind");
    return 1;
  }

  socklen_t len = sizeof(addr);
  if (getsockname(receiver, (struct sockaddr*)&addr, &len) != 0) {
    perror("getsockname");
    return 1;
  }

  const char msg[] = "hello";
  if (sendto(sender, msg, sizeof(msg) - 1, 0, (struct sockaddr*)&addr,
             sizeof(addr)) != (ssize_t)(sizeof(msg) - 1)) {
    perror("sendto");
    return 1;
  }

  char peek_buf[16] = {0};
  char recv_buf[16] = {0};
  ssize_t peek_len =
      recvfrom(receiver, peek_buf, sizeof(peek_buf), MSG_PEEK, NULL, NULL);
  if (peek_len != (ssize_t)(sizeof(msg) - 1) ||
      memcmp(peek_buf, msg, sizeof(msg) - 1) != 0) {
    fprintf(stderr, "MSG_PEEK: expected `%s`, got %zd bytes `%.*s`\n", msg,
            peek_len, peek_len > 0 ? (int)peek_len : 0, peek_buf);
    return 1;
  }

  ssize_t first_len =
      recvfrom(receiver, recv_buf, sizeof(recv_buf), 0, NULL, NULL);
  if (first_len != (ssize_t)(sizeof(msg) - 1) ||
      memcmp(recv_buf, msg, sizeof(msg) - 1) != 0) {
    fprintf(stderr, "first recv: expected `%s`, got %zd bytes\n", msg,
            first_len);
    return 1;
  }

  if (fcntl(receiver, F_SETFL, fcntl(receiver, F_GETFL, 0) | O_NONBLOCK) != 0) {
    perror("fcntl(O_NONBLOCK)");
    return 1;
  }

  char drain_buf[16];
  if (recvfrom(receiver, drain_buf, sizeof(drain_buf), 0, NULL, NULL) >= 0 ||
      (errno != EAGAIN && errno != EWOULDBLOCK)) {
    fprintf(stderr,
            "second recv: expected EAGAIN after draining peeked datagram\n");
    return 1;
  }

  close(sender);
  close(receiver);
  puts("UDP MSG_PEEK leaves datagram queued");
  return 0;
}
