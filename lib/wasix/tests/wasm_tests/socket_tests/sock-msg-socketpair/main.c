//#ExpectedStdout: sock_msg socketpair works
#include <stdint.h>
#include <stdio.h>
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

static int32_t test_imported_sock_send_msg(
    int32_t fd, int32_t si_data, int32_t si_data_len, int32_t si_flags,
    int32_t addr, int32_t si_control, int32_t si_control_len,
    int32_t ret_data_len) __attribute__((__import_module__("wasix_32v1"),
                                         __import_name__("sock_send_msg")));

static int32_t test_imported_sock_recv_msg(
    int32_t fd, int32_t ri_data, int32_t ri_data_len, int32_t ri_flags,
    int32_t addr, int32_t ro_control, int32_t ro_control_len,
    int32_t ro_data_len, int32_t ro_flags, int32_t ro_control_len_out)
    __attribute__((__import_module__("wasix_32v1"),
                   __import_name__("sock_recv_msg")));

static int check_payload_round_trip(void) {
  int sockets[2];
  if (socketpair(AF_UNIX, SOCK_STREAM, 0, sockets) != 0) {
    perror("socketpair");
    return 1;
  }

  const char payload[] = "hello from sock_msg socketpair";
  __wasi_ciovec_t send_iov = {
      .buf = (const uint8_t*)payload,
      .buf_len = strlen(payload),
  };
  __wasi_size_t sent = 0;
  __wasi_errno_t err = (__wasi_errno_t)test_imported_sock_send_msg(
      sockets[0], (int32_t)(intptr_t)&send_iov, 1, 0, 0, 0, 0,
      (int32_t)(intptr_t)&sent);

  if (err != __WASI_ERRNO_SUCCESS) {
    fprintf(stderr, "sock_send_msg failed: %u\n", err);
    close(sockets[0]);
    close(sockets[1]);
    return 1;
  }
  if (sent != strlen(payload)) {
    fprintf(stderr, "unexpected sent length: %lu\n", sent);
    close(sockets[0]);
    close(sockets[1]);
    return 1;
  }

  char buf[64] = {0};
  __wasi_iovec_t recv_iov = {
      .buf = (uint8_t*)buf,
      .buf_len = sizeof(buf) - 1,
  };
  uint8_t control[64] = {0};
  __wasi_size_t received = 0;
  __wasi_roflags_t flags = 0xffff;
  __wasi_size_t control_len = 99;
  err = (__wasi_errno_t)test_imported_sock_recv_msg(
      sockets[1], (int32_t)(intptr_t)&recv_iov, 1, 0, 0,
      (int32_t)(intptr_t)control, sizeof(control), (int32_t)(intptr_t)&received,
      (int32_t)(intptr_t)&flags, (int32_t)(intptr_t)&control_len);

  if (err != __WASI_ERRNO_SUCCESS) {
    fprintf(stderr, "sock_recv_msg failed: %u\n", err);
    close(sockets[0]);
    close(sockets[1]);
    return 1;
  }
  if (received != strlen(payload) || strcmp(buf, payload) != 0) {
    fprintf(stderr, "unexpected payload: %s\n", buf);
    close(sockets[0]);
    close(sockets[1]);
    return 1;
  }
  if (flags != 0 || control_len != 0) {
    fprintf(stderr, "unexpected recv metadata: flags=%u control_len=%lu\n",
            flags, control_len);
    close(sockets[0]);
    close(sockets[1]);
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
      sockets[0], (int32_t)(intptr_t)&iov, 1, 0, 0, (int32_t)(intptr_t)control,
      sizeof(control), (int32_t)(intptr_t)&sent);

  if (err != __WASI_ERRNO_NOTSUP) {
    fprintf(stderr, "expected NOTSUP for control data, got: %u\n", err);
    close(sockets[0]);
    close(sockets[1]);
    return 1;
  }

  close(sockets[0]);
  close(sockets[1]);
  return 0;
}

int main(void) {
  if (check_payload_round_trip() != 0) return 1;
  if (check_control_send_is_rejected() != 0) return 1;

  puts("sock_msg socketpair works");
  return 0;
}
