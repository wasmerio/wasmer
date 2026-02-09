#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <wasi/api_wasix.h>

static __wasi_fd_t open_tcp_socket(void)
{
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(
        __WASI_ADDRESS_FAMILY_INET4,
        __WASI_SOCK_TYPE_SOCKET_STREAM,
        __WASI_SOCK_PROTO_TCP,
        &fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    return fd;
}

int main(void)
{
    printf("sock_set_opt_size repro\n");
    __wasi_fd_t fd = open_tcp_socket();

    __wasi_errno_t err = __wasi_sock_set_opt_size(
        fd,
        __WASI_SOCK_OPTION_RECV_BUF_SIZE,
        8192);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_set_opt_size(
        fd,
        __WASI_SOCK_OPTION_SEND_BUF_SIZE,
        16384);
    assert(err == __WASI_ERRNO_SUCCESS);

    printf("All tests passed!\n");
    return 0;
}
