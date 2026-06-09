//#ExpectedStdout: sock_send_msg works
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

#ifndef __WASI_SOCK_CMSG_LEVEL_SOCKET
#define __WASI_SOCK_CMSG_LEVEL_SOCKET UINT16_C(0)
#define __WASI_SOCK_CMSG_TYPE_RIGHTS UINT16_C(0)
typedef struct __wasi_sock_cmsg_t {
  __wasi_size_t cmsg_len;
  uint16_t cmsg_level;
  uint16_t cmsg_type;
} __wasi_sock_cmsg_t;
#endif

static int32_t test_imported_sock_send_msg(int32_t fd,
                                           int32_t si_data,
                                           int32_t si_data_len,
                                           int32_t si_flags,
                                           int32_t addr,
                                           int32_t si_control,
                                           int32_t si_control_len,
                                           int32_t ret_data_len)
    __attribute__((__import_module__("wasix_32v1"),
                   __import_name__("sock_send_msg")));

static int check_payload_only_send(void) {
  int sockets[2];
  if (socketpair(AF_UNIX, SOCK_STREAM, 0, sockets) != 0) {
    perror("socketpair");
    return 1;
  }

  const char payload[] = "hello from sock_send_msg";
  __wasi_ciovec_t iov = {
    .buf = (const uint8_t*)payload,
    .buf_len = strlen(payload),
  };
  __wasi_size_t sent = 0;
  __wasi_errno_t err = (__wasi_errno_t)test_imported_sock_send_msg(
      sockets[0], (int32_t)(intptr_t)&iov, 1, 0, 0, 0, 0,
      (int32_t)(intptr_t)&sent);

  if (err != __WASI_ERRNO_SUCCESS) {
    fprintf(stderr, "sock_send_msg failed: %u\n", err);
    return 1;
  }
  if (sent != strlen(payload)) {
    fprintf(stderr, "unexpected sent length: %lu\n", sent);
    return 1;
  }

  char buf[64] = {0};
  ssize_t received = recv(sockets[1], buf, sizeof(buf) - 1, 0);
  if (received < 0) {
    perror("recv");
    return 1;
  }
  if ((size_t)received != strlen(payload) || strcmp(buf, payload) != 0) {
    fprintf(stderr, "unexpected payload: %s\n", buf);
    return 1;
  }

  close(sockets[0]);
  close(sockets[1]);
  return 0;
}

static int check_control_send_is_rejected(void) {
  int sockets[2];
  if (socketpair(AF_UNIX, SOCK_STREAM, 0, sockets) != 0) {
    perror("socketpair");
    return 1;
  }

  const char payload[] = "x";
  __wasi_ciovec_t iov = {
    .buf = (const uint8_t*)payload,
    .buf_len = strlen(payload),
  };

  uint8_t control[sizeof(__wasi_sock_cmsg_t) + sizeof(__wasi_fd_t)] = {0};
  __wasi_sock_cmsg_t* cmsg = (__wasi_sock_cmsg_t*)control;
  __wasi_fd_t fd = (__wasi_fd_t)sockets[1];
  cmsg->cmsg_len = sizeof(control);
  cmsg->cmsg_level = __WASI_SOCK_CMSG_LEVEL_SOCKET;
  cmsg->cmsg_type = __WASI_SOCK_CMSG_TYPE_RIGHTS;
  memcpy(control + sizeof(__wasi_sock_cmsg_t), &fd, sizeof(fd));

  __wasi_size_t sent = 99;
  __wasi_errno_t err = (__wasi_errno_t)test_imported_sock_send_msg(
      sockets[0], (int32_t)(intptr_t)&iov, 1, 0, 0,
      (int32_t)(intptr_t)control, sizeof(control), (int32_t)(intptr_t)&sent);

  if (err != __WASI_ERRNO_NOTSUP) {
    fprintf(stderr, "expected NOTSUP for control data, got: %u\n", err);
    return 1;
  }

  close(sockets[0]);
  close(sockets[1]);
  return 0;
}

int main(void) {
  if (check_payload_only_send() != 0)
    return 1;
  if (check_control_send_is_rejected() != 0)
    return 1;

  puts("sock_send_msg works");
  return 0;
}
