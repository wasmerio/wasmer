//#ExpectedStdout: zero-length UDP datagram poll_oneoff is readable

#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

int main(void) {
  int fd = socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);
  if (fd < 0) {
    perror("socket");
    return 1;
  }

  struct sockaddr_in addr = {0};
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr.sin_port = 0;
  if (bind(fd, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("bind");
    return 1;
  }

  socklen_t len = sizeof(addr);
  if (getsockname(fd, (struct sockaddr*)&addr, &len) != 0) {
    perror("getsockname");
    return 1;
  }

  if (sendto(fd, "", 0, 0, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    perror("sendto zero-length");
    return 1;
  }

  __wasi_subscription_t sub = {
      .userdata = 42,
      .u = {.tag = __WASI_EVENTTYPE_FD_READ,
            .u = {.fd_read = {.file_descriptor = fd}}}};
  __wasi_event_t ev = {0};
  __wasi_size_t nevents = 0;
  __wasi_errno_t err = __wasi_poll_oneoff(&sub, &ev, 1, &nevents);
  if (err != __WASI_ERRNO_SUCCESS) {
    fprintf(stderr, "poll_oneoff failed: %u\n", err);
    return 1;
  }
  if (nevents != 1 || ev.type != __WASI_EVENTTYPE_FD_READ ||
      ev.error != __WASI_ERRNO_SUCCESS) {
    fprintf(stderr,
            "unexpected poll_oneoff event: nevents=%u type=%u error=%u\n",
            (unsigned)nevents, ev.type, ev.error);
    return 1;
  }
  if ((ev.fd_readwrite.flags & __WASI_EVENTRWFLAGS_FD_READWRITE_HANGUP) != 0) {
    fprintf(stderr, "zero-length UDP datagram reported as hangup (flags=%u)\n",
            ev.fd_readwrite.flags);
    return 1;
  }

  char byte;
  ssize_t n = recvfrom(fd, &byte, sizeof(byte), 0, NULL, NULL);
  if (n != 0) {
    fprintf(stderr, "expected zero-length recvfrom, got %zd\n", n);
    return 1;
  }

  close(fd);
  puts("zero-length UDP datagram poll_oneoff is readable");
  return 0;
}
