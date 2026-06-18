//#ExpectedStdout: connected UDP peer and sendto work

#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  int receiver = socket(AF_INET, SOCK_DGRAM, 0);
  if (receiver < 0) {
    perror("socket(receiver)");
    return 1;
  }

  int sender = socket(AF_INET, SOCK_DGRAM, 0);
  if (sender < 0) {
    perror("socket(sender)");
    close(receiver);
    return 1;
  }

  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr.sin_port = 0;
  if (bind(receiver, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("bind(receiver)");
    close(sender);
    close(receiver);
    return 1;
  }

  socklen_t len = sizeof(addr);
  if (getsockname(receiver, (struct sockaddr*)&addr, &len) != 0) {
    perror("getsockname(receiver)");
    close(sender);
    close(receiver);
    return 1;
  }

  if (connect(sender, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("connect(sender)");
    close(sender);
    close(receiver);
    return 1;
  }

  struct sockaddr_in peer;
  memset(&peer, 0, sizeof(peer));
  socklen_t peer_len = sizeof(peer);
  if (getpeername(sender, (struct sockaddr*)&peer, &peer_len) != 0) {
    perror("getpeername(sender)");
    close(sender);
    close(receiver);
    return 1;
  }

  if (peer.sin_family != AF_INET || peer.sin_port != addr.sin_port ||
      peer.sin_addr.s_addr != addr.sin_addr.s_addr) {
    fprintf(stderr, "unexpected peer: family=%d port=%u addr=%08x\n",
            peer.sin_family, ntohs(peer.sin_port), ntohl(peer.sin_addr.s_addr));
    close(sender);
    close(receiver);
    return 1;
  }

  struct sockaddr_in sender_addr;
  memset(&sender_addr, 0, sizeof(sender_addr));
  socklen_t sender_addr_len = sizeof(sender_addr);
  if (getsockname(sender, (struct sockaddr*)&sender_addr, &sender_addr_len) !=
      0) {
    perror("getsockname(sender)");
    close(sender);
    close(receiver);
    return 1;
  }

  ssize_t written = send(sender, "hello", 5, 0);
  if (written != 5) {
    fprintf(stderr, "expected sendto to write 5 bytes, got %zd errno=%d (%s)\n",
            written, errno, strerror(errno));
    close(sender);
    close(receiver);
    return 1;
  }

  char buf[16] = {0};
  struct sockaddr_in peer_from;
  socklen_t peer_from_len = sizeof(peer_from);
  memset(&peer_from, 0, sizeof(peer_from));
  ssize_t nread =
      recvfrom(receiver, buf, sizeof(buf), 0, &peer_from, &peer_from_len);
  if (nread != 5 || memcmp(buf, "hello", 5) != 0) {
    fprintf(stderr,
            "expected one 5-byte datagram `hello`, got %zd bytes: %.*s\n",
            nread, nread > 0 ? (int)nread : 0, buf);
    close(sender);
    close(receiver);
    return 1;
  }

  if (peer_from_len != sizeof(peer_from) || peer_from.sin_family != AF_INET ||
      peer_from.sin_port != sender_addr.sin_port ||
      // sender has 0.0.0.0 and recvfrom() reports 127.0.0.1
      peer_from.sin_addr.s_addr != htonl(INADDR_LOOPBACK) ||
      sender_addr.sin_addr.s_addr != htonl(INADDR_ANY)) {
    fprintf(stderr,
            "unexpected recvfrom/source state: peer_from family=%d port=%u "
            "addr=%08x, sender family=%d port=%u addr=%08x\n",
            peer_from.sin_family, ntohs(peer_from.sin_port),
            ntohl(peer_from.sin_addr.s_addr), sender_addr.sin_family,
            ntohs(sender_addr.sin_port), ntohl(sender_addr.sin_addr.s_addr));
    close(sender);
    close(receiver);
    return 1;
  }

  close(sender);
  close(receiver);
  puts("connected UDP peer and sendto work");
  return 0;
}
