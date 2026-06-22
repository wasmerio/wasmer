//#ExpectedStdout: UDP oversize payload returns EMSGSIZE
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>
#include <wasi/api_wasi.h>
#include <wasi/api_wasix.h>

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

static int open_udp_pair(int* receiver, int* sender,
                         struct sockaddr_in* receiver_addr) {
  *receiver = socket(AF_INET, SOCK_DGRAM, 0);
  if (*receiver < 0) {
    perror("socket(receiver)");
    return 1;
  }

  memset(receiver_addr, 0, sizeof(*receiver_addr));
  receiver_addr->sin_family = AF_INET;
  receiver_addr->sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  receiver_addr->sin_port = 0;
  if (bind(*receiver, (struct sockaddr*)receiver_addr,
           sizeof(*receiver_addr)) != 0) {
    perror("bind(receiver)");
    close(*receiver);
    return 1;
  }

  socklen_t len = sizeof(*receiver_addr);
  if (getsockname(*receiver, (struct sockaddr*)receiver_addr, &len) != 0) {
    perror("getsockname(receiver)");
    close(*receiver);
    return 1;
  }

  *sender = socket(AF_INET, SOCK_DGRAM, 0);
  if (*sender < 0) {
    perror("socket(sender)");
    close(*receiver);
    return 1;
  }

  if (connect(*sender, (struct sockaddr*)receiver_addr,
              sizeof(*receiver_addr)) != 0) {
    perror("connect(sender)");
    close(*sender);
    close(*receiver);
    return 1;
  }
  return 0;
}

static int expect_wasi_msgsize(__wasi_errno_t err, const char* label) {
  if (err != __WASI_ERRNO_MSGSIZE) {
    fprintf(stderr, "%s: expected WASI MSGSIZE (%u), got %u\n", label,
            __WASI_ERRNO_MSGSIZE, err);
    return 1;
  }
  return 0;
}

static int expect_posix_msgsize(const char* label) {
  if (errno != EMSGSIZE) {
    fprintf(stderr, "%s: expected EMSGSIZE (%d), got errno=%d (%s)\n", label,
            EMSGSIZE, errno, strerror(errno));
    return 1;
  }
  return 0;
}

static int test_single_buffer_oversize(
    int sender, const struct sockaddr_in* receiver_addr) {
  static uint8_t bigbuf[128 * 1024];
  memset(bigbuf, 0x42, sizeof(bigbuf));

  __wasi_ciovec_t iov = {.buf = bigbuf, .buf_len = sizeof(bigbuf)};
  __wasi_size_t nsent = 0;
  __wasi_errno_t err = __wasi_sock_send(sender, &iov, 1, 0, &nsent);
  if (expect_wasi_msgsize(err, "single-buffer send") != 0) {
    return 1;
  }

  __wasi_addr_port_t dest;
  sockaddr_in_to_wasi(receiver_addr, &dest);
  err = __wasi_sock_send_to(sender, &iov, 1, 0, &dest, &nsent);
  if (expect_wasi_msgsize(err, "single-buffer sendto") != 0) {
    return 1;
  }

  errno = 0;
  ssize_t written = send(sender, (const char*)bigbuf, sizeof(bigbuf), 0);
  if (written >= 0) {
    fprintf(stderr, "single-buffer POSIX send: expected failure, got %zd\n",
            written);
    return 1;
  }
  return expect_posix_msgsize("single-buffer POSIX send");
}

static int test_iovec_sum_oversize(int sender,
                                   const struct sockaddr_in* receiver_addr) {
  static uint8_t part1[35000];
  static uint8_t part2[30508];
  memset(part1, 0x11, sizeof(part1));
  memset(part2, 0x22, sizeof(part2));

  __wasi_ciovec_t iovs[2] = {
      {.buf = part1, .buf_len = sizeof(part1)},
      {.buf = part2, .buf_len = sizeof(part2)},
  };
  __wasi_size_t nsent = 0;
  __wasi_errno_t err = __wasi_sock_send(sender, iovs, 2, 0, &nsent);
  if (expect_wasi_msgsize(err, "multi-iovec send") != 0) {
    return 1;
  }

  __wasi_addr_port_t dest;
  sockaddr_in_to_wasi(receiver_addr, &dest);
  err = __wasi_sock_send_to(sender, iovs, 2, 0, &dest, &nsent);
  if (expect_wasi_msgsize(err, "multi-iovec sendto") != 0) {
    return 1;
  }

  __wasi_size_t nwritten = 0;
  err = __wasi_fd_write(sender, iovs, 2, &nwritten);
  return expect_wasi_msgsize(err, "multi-iovec writev");
}

int main(void) {
  int receiver;
  int sender;
  struct sockaddr_in receiver_addr;
  if (open_udp_pair(&receiver, &sender, &receiver_addr) != 0) {
    return 1;
  }

  if (test_single_buffer_oversize(sender, &receiver_addr) != 0) {
    close(sender);
    close(receiver);
    return 1;
  }
  if (test_iovec_sum_oversize(sender, &receiver_addr) != 0) {
    close(sender);
    close(receiver);
    return 1;
  }

  close(sender);
  close(receiver);
  puts("UDP oversize payload returns EMSGSIZE");
  return 0;
}
