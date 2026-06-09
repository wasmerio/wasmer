//#ExpectedStdout: sock_msg works
#include <arpa/inet.h>
#include <netinet/in.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

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

static void sockaddr_in_to_wasi(const struct sockaddr_in* addr,
                                __wasi_addr_port_t* wasi_addr) {
  memset(wasi_addr, 0, sizeof(*wasi_addr));
  wasi_addr->tag = __WASI_ADDRESS_FAMILY_INET4;
  wasi_addr->u.inet4.port = ntohs(addr->sin_port);
  memcpy(&wasi_addr->u.inet4.addr, &addr->sin_addr.s_addr,
         sizeof(addr->sin_addr.s_addr));
}

int main(void) {
  int sender = socket(AF_INET, SOCK_DGRAM, 0);
  if (sender < 0) {
    perror("sender socket");
    return 1;
  }

  int receiver = socket(AF_INET, SOCK_DGRAM, 0);
  if (receiver < 0) {
    perror("receiver socket");
    close(sender);
    return 1;
  }

  struct sockaddr_in receiver_addr = {0};
  receiver_addr.sin_family = AF_INET;
  receiver_addr.sin_port = htons(0);
  receiver_addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  if (bind(receiver, (struct sockaddr*)&receiver_addr, sizeof(receiver_addr)) !=
      0) {
    perror("receiver bind");
    close(sender);
    close(receiver);
    return 1;
  }

  socklen_t receiver_addr_len = sizeof(receiver_addr);
  if (getsockname(receiver, (struct sockaddr*)&receiver_addr,
                  &receiver_addr_len) != 0) {
    perror("receiver getsockname");
    close(sender);
    close(receiver);
    return 1;
  }

  __wasi_addr_port_t wasi_receiver_addr;
  sockaddr_in_to_wasi(&receiver_addr, &wasi_receiver_addr);

  const char payload[] = "hello from sock_msg";
  __wasi_ciovec_t send_iov = {
      .buf = (const uint8_t*)payload,
      .buf_len = strlen(payload),
  };
  __wasi_size_t sent = 0;
  __wasi_errno_t err = (__wasi_errno_t)test_imported_sock_send_msg(
      sender, (int32_t)(intptr_t)&send_iov, 1, 0,
      (int32_t)(intptr_t)&wasi_receiver_addr, 0, 0, (int32_t)(intptr_t)&sent);

  if (err != __WASI_ERRNO_SUCCESS) {
    fprintf(stderr, "sock_send_msg failed: %u\n", err);
    close(sender);
    close(receiver);
    return 1;
  }
  if (sent != strlen(payload)) {
    fprintf(stderr, "unexpected sent length: %lu\n", sent);
    close(sender);
    close(receiver);
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
      receiver, (int32_t)(intptr_t)&recv_iov, 1, 0, 0,
      (int32_t)(intptr_t)control, sizeof(control), (int32_t)(intptr_t)&received,
      (int32_t)(intptr_t)&flags, (int32_t)(intptr_t)&control_len);

  if (err != __WASI_ERRNO_SUCCESS) {
    fprintf(stderr, "sock_recv_msg failed: %u\n", err);
    close(sender);
    close(receiver);
    return 1;
  }
  if (received != strlen(payload) || strcmp(buf, payload) != 0) {
    fprintf(stderr, "unexpected payload: %s\n", buf);
    close(sender);
    close(receiver);
    return 1;
  }
  if (flags != 0 || control_len != 0) {
    fprintf(stderr, "unexpected recv metadata: flags=%u control_len=%lu\n",
            flags, control_len);
    close(sender);
    close(receiver);
    return 1;
  }

  close(sender);
  close(receiver);
  puts("sock_msg works");
  return 0;
}
