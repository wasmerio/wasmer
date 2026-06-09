//#ExpectedStdout: sock_recv_msg works
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

static int32_t test_imported_sock_recv_msg(int32_t fd,
                                           int32_t ri_data,
                                           int32_t ri_data_len,
                                           int32_t ri_flags,
                                           int32_t addr,
                                           int32_t ro_control,
                                           int32_t ro_control_len,
                                           int32_t ro_data_len,
                                           int32_t ro_flags,
                                           int32_t ro_control_len_out)
    __attribute__((__import_module__("wasix_32v1"),
                   __import_name__("sock_recv_msg")));

int main(void) {
  int sockets[2];
  if (socketpair(AF_UNIX, SOCK_STREAM, 0, sockets) != 0) {
    perror("socketpair");
    return 1;
  }

  const char payload[] = "hello from sock_recv_msg";
  ssize_t sent = send(sockets[0], payload, strlen(payload), 0);
  if (sent < 0) {
    perror("send");
    return 1;
  }
  if ((size_t)sent != strlen(payload)) {
    fprintf(stderr, "unexpected send length: %zd\n", sent);
    return 1;
  }

  char buf[64] = {0};
  __wasi_iovec_t iov = {
    .buf = (uint8_t*)buf,
    .buf_len = sizeof(buf) - 1,
  };
  uint8_t control[64] = {0};
  __wasi_size_t received = 0;
  __wasi_roflags_t flags = 0xffff;
  __wasi_size_t control_len = 99;

  __wasi_errno_t err = (__wasi_errno_t)test_imported_sock_recv_msg(
      sockets[1], (int32_t)(intptr_t)&iov, 1, 0, 0,
      (int32_t)(intptr_t)control, sizeof(control), (int32_t)(intptr_t)&received,
      (int32_t)(intptr_t)&flags, (int32_t)(intptr_t)&control_len);

  if (err != __WASI_ERRNO_SUCCESS) {
    fprintf(stderr, "sock_recv_msg failed: %u\n", err);
    return 1;
  }
  if (received != strlen(payload)) {
    fprintf(stderr, "unexpected received length: %lu\n", received);
    return 1;
  }
  if (strcmp(buf, payload) != 0) {
    fprintf(stderr, "unexpected payload: %s\n", buf);
    return 1;
  }
  if (flags != 0) {
    fprintf(stderr, "unexpected recv flags: %u\n", flags);
    return 1;
  }
  if (control_len != 0) {
    fprintf(stderr, "unexpected control length: %lu\n", control_len);
    return 1;
  }

  close(sockets[0]);
  close(sockets[1]);
  puts("sock_recv_msg works");
  return 0;
}
