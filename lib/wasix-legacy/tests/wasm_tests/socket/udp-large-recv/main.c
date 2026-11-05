//#Ignored: flaky unstable test (#6785)
//#ExpectedStdout: large UDP datagram receive works

#include <arpa/inet.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#define PAYLOAD_SIZE 20480

static uint8_t sendbuf[PAYLOAD_SIZE];
static uint8_t recvbuf[PAYLOAD_SIZE];

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

  for (size_t i = 0; i < PAYLOAD_SIZE; ++i) {
    sendbuf[i] = (uint8_t)(i & 0xff);
  }

  if (sendto(sender, sendbuf, PAYLOAD_SIZE, 0, (struct sockaddr*)&addr,
             sizeof(addr)) != PAYLOAD_SIZE) {
    perror("sendto");
    return 1;
  }

  ssize_t nread = recvfrom(receiver, recvbuf, sizeof(recvbuf), 0, NULL, NULL);
  if (nread != PAYLOAD_SIZE) {
    fprintf(stderr, "expected %d-byte datagram, got %zd\n", PAYLOAD_SIZE,
            nread);
    return 1;
  }
  if (memcmp(sendbuf, recvbuf, PAYLOAD_SIZE) != 0) {
    fprintf(stderr, "payload mismatch\n");
    return 1;
  }

  close(sender);
  close(receiver);
  puts("large UDP datagram receive works");
  return 0;
}
