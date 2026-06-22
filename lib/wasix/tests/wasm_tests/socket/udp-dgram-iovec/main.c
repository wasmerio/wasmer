//#ExpectedStdout: UDP multi-iovec datagram coalescing works
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>
#include <wasi/api_wasi.h>
#include <wasi/api_wasix.h>

static const char EXPECTED[] = "hello world!";

static void sockaddr_in_to_wasi(const struct sockaddr_in* in,
                                __wasi_addr_port_t* out) {
  memset(out, 0, sizeof(*out));
  out->tag = __WASI_ADDRESS_FAMILY_INET4;
  unsigned char* octs = (unsigned char*)&out->u;
  uint16_t port = ntohs(in->sin_port);
  octs[0] = (unsigned char)(port & 0xff);
  octs[1] = (unsigned char)((port >> 8) & 0xff);
  memcpy(&octs[2], &in->sin_addr, 4);
}

static int setup_receiver(int* receiver, struct sockaddr_in* addr) {
  *receiver = socket(AF_INET, SOCK_DGRAM, 0);
  if (*receiver < 0) {
    perror("socket(receiver)");
    return 1;
  }

  memset(addr, 0, sizeof(*addr));
  addr->sin_family = AF_INET;
  addr->sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr->sin_port = 0;
  if (bind(*receiver, (struct sockaddr*)addr, sizeof(*addr)) != 0) {
    perror("bind(receiver)");
    close(*receiver);
    return 1;
  }

  socklen_t len = sizeof(*addr);
  if (getsockname(*receiver, (struct sockaddr*)addr, &len) != 0) {
    perror("getsockname(receiver)");
    close(*receiver);
    return 1;
  }
  return 0;
}

static int expect_one_datagram(int receiver, const char* label) {
  char buf[32] = {0};
  ssize_t nread = recv(receiver, buf, sizeof(buf), 0);
  if (nread != (ssize_t)sizeof(EXPECTED) - 1) {
    fprintf(stderr, "%s: expected %zu-byte datagram, got %zd bytes\n", label,
            sizeof(EXPECTED) - 1, nread);
    return 1;
  }
  if (memcmp(buf, EXPECTED, sizeof(EXPECTED) - 1) != 0) {
    fprintf(stderr, "%s: unexpected payload: %.*s\n", label, (int)nread, buf);
    return 1;
  }
  return 0;
}

static void make_iovecs(__wasi_ciovec_t iovs[3]) {
  static const char part1[] = "hel";
  static const char part2[] = "lo ";
  static const char part3[] = "world!";
  iovs[0] = (__wasi_ciovec_t){.buf = (uint8_t*)part1, .buf_len = 3};
  iovs[1] = (__wasi_ciovec_t){.buf = (uint8_t*)part2, .buf_len = 3};
  iovs[2] = (__wasi_ciovec_t){.buf = (uint8_t*)part3, .buf_len = 6};
}

static int test_sendto_iovec(int receiver, const struct sockaddr_in* addr) {
  int sender = socket(AF_INET, SOCK_DGRAM, 0);
  if (sender < 0) {
    perror("socket(sendto)");
    return 1;
  }

  __wasi_ciovec_t iovs[3];
  make_iovecs(iovs);

  __wasi_addr_port_t dest;
  sockaddr_in_to_wasi(addr, &dest);

  __wasi_size_t nsent = 0;
  __wasi_errno_t err = __wasi_sock_send_to(sender, iovs, 3, 0, &dest, &nsent);
  if (err != __WASI_ERRNO_SUCCESS) {
    fprintf(stderr, "sendto iovec: expected success, got wasi errno %u\n", err);
    close(sender);
    return 1;
  }
  if (nsent != sizeof(EXPECTED) - 1) {
    fprintf(stderr, "sendto iovec: expected %zu bytes sent, got %u\n",
            sizeof(EXPECTED) - 1, (unsigned)nsent);
    close(sender);
    return 1;
  }

  int rc = expect_one_datagram(receiver, "sendto iovec");
  close(sender);
  return rc;
}

static int test_send_iovec(int receiver, const struct sockaddr_in* addr) {
  int sender = socket(AF_INET, SOCK_DGRAM, 0);
  if (sender < 0) {
    perror("socket(send)");
    return 1;
  }

  if (connect(sender, (struct sockaddr*)addr, sizeof(*addr)) != 0) {
    perror("connect(sender)");
    close(sender);
    return 1;
  }

  __wasi_ciovec_t iovs[3];
  make_iovecs(iovs);

  __wasi_size_t nsent = 0;
  __wasi_errno_t err = __wasi_sock_send(sender, iovs, 3, 0, &nsent);
  if (err != __WASI_ERRNO_SUCCESS) {
    fprintf(stderr, "send iovec: expected success, got wasi errno %u\n", err);
    close(sender);
    return 1;
  }
  if (nsent != sizeof(EXPECTED) - 1) {
    fprintf(stderr, "send iovec: expected %zu bytes sent, got %u\n",
            sizeof(EXPECTED) - 1, (unsigned)nsent);
    close(sender);
    return 1;
  }

  int rc = expect_one_datagram(receiver, "send iovec");
  close(sender);
  return rc;
}

static int test_writev_iovec(int receiver, const struct sockaddr_in* addr) {
  int sender = socket(AF_INET, SOCK_DGRAM, 0);
  if (sender < 0) {
    perror("socket(writev)");
    return 1;
  }

  if (connect(sender, (struct sockaddr*)addr, sizeof(*addr)) != 0) {
    perror("connect(writev sender)");
    close(sender);
    return 1;
  }

  __wasi_ciovec_t iovs[3];
  make_iovecs(iovs);

  __wasi_size_t nwritten = 0;
  __wasi_errno_t err = __wasi_fd_write(sender, iovs, 3, &nwritten);
  if (err != __WASI_ERRNO_SUCCESS) {
    fprintf(stderr, "writev iovec: expected success, got wasi errno %u\n", err);
    close(sender);
    return 1;
  }
  if (nwritten != sizeof(EXPECTED) - 1) {
    fprintf(stderr, "writev iovec: expected %zu bytes written, got %u\n",
            sizeof(EXPECTED) - 1, (unsigned)nwritten);
    close(sender);
    return 1;
  }

  int rc = expect_one_datagram(receiver, "writev iovec");
  close(sender);
  return rc;
}

int main(void) {
  int receiver;
  struct sockaddr_in addr;
  if (setup_receiver(&receiver, &addr) != 0) {
    return 1;
  }

  if (test_sendto_iovec(receiver, &addr) != 0) {
    close(receiver);
    return 1;
  }
  if (test_send_iovec(receiver, &addr) != 0) {
    close(receiver);
    return 1;
  }
  if (test_writev_iovec(receiver, &addr) != 0) {
    close(receiver);
    return 1;
  }

  close(receiver);
  puts("UDP multi-iovec datagram coalescing works");
  return 0;
}
