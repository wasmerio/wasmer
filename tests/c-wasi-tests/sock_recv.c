#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

#ifndef __WASI_SOCK_RECV_INPUT_DONT_WAIT
#define __WASI_SOCK_RECV_INPUT_DONT_WAIT ((__wasi_riflags_t)(1 << 3))
#endif

static int failures = 0;

static void expect_errno(const char *name, __wasi_errno_t got,
                         __wasi_errno_t expect)
{
    if (got != expect) {
        fprintf(stderr, "%s: expected %u, got %u\n", name, expect, got);
        failures++;
    }
}

static void set_ipv4_addr_port_le(__wasi_addr_port_t *addr, uint16_t port,
                                  uint8_t a, uint8_t b, uint8_t c, uint8_t d)
{
    memset(addr, 0, sizeof(*addr));
    addr->tag = __WASI_ADDRESS_FAMILY_INET4;
    unsigned char *octs = (unsigned char *)&addr->u;
    octs[0] = (unsigned char)(port & 0xff);
    octs[1] = (unsigned char)((port >> 8) & 0xff);
    octs[2] = a;
    octs[3] = b;
    octs[4] = c;
    octs[5] = d;
}

static uint16_t port_from_addr_be(const __wasi_addr_port_t *addr)
{
    const unsigned char *octs = (const unsigned char *)&addr->u;
    return (uint16_t)(((uint16_t)octs[0] << 8) | (uint16_t)octs[1]);
}

static void open_connected_tcp(__wasi_fd_t *client_fd, __wasi_fd_t *server_fd,
                               __wasi_fd_t *accepted_fd)
{
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          server_fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t bind_addr;
    set_ipv4_addr_port_le(&bind_addr, 0, 127, 0, 0, 1);
    err = __wasi_sock_bind(*server_fd, &bind_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_listen(*server_fd, 1);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t local_addr;
    err = __wasi_sock_addr_local(*server_fd, &local_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint16_t port = port_from_addr_be(&local_addr);
    assert(port != 0);

    err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                           __WASI_SOCK_TYPE_SOCKET_STREAM,
                           __WASI_SOCK_PROTO_TCP,
                           client_fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t connect_addr;
    set_ipv4_addr_port_le(&connect_addr, port, 127, 0, 0, 1);
    err = __wasi_sock_connect(*client_fd, &connect_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t accepted_addr;
    err = __wasi_sock_accept_v2(*server_fd, 0, accepted_fd, &accepted_addr);
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void close_pair(__wasi_fd_t client_fd, __wasi_fd_t server_fd,
                       __wasi_fd_t accepted_fd)
{
    close(accepted_fd);
    close(client_fd);
    close(server_fd);
}

static void send_all(__wasi_fd_t fd, const void *data, size_t len)
{
    const uint8_t *bytes = (const uint8_t *)data;
    size_t offset = 0;
    while (offset < len) {
        __wasi_ciovec_t iov = {
            .buf = (const uint8_t *)(bytes + offset),
            .buf_len = len - offset,
        };
        __wasi_size_t sent = 0;
        __wasi_errno_t err = __wasi_sock_send(fd, &iov, 1, 0, &sent);
        assert(err == __WASI_ERRNO_SUCCESS);
        assert(sent > 0);
        offset += sent;
    }
}

static void test_invalid_fd(void)
{
    // From LTP recv01: EBADF on invalid fd.
    printf("Test 1: invalid fd\n");
    uint8_t buf[4] = {0};
    __wasi_iovec_t iov = {.buf = buf, .buf_len = sizeof(buf)};
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_errno_t err =
        __wasi_sock_recv(9999, &iov, 1, 0, &nread, &roflags);
    expect_errno("invalid fd", err, __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    // From LTP recv01: ENOTSOCK on non-socket fd.
    printf("Test 2: not a socket\n");
    int fd = open("sock_recv_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    uint8_t buf[4] = {0};
    __wasi_iovec_t iov = {.buf = buf, .buf_len = sizeof(buf)};
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_errno_t err =
        __wasi_sock_recv((__wasi_fd_t)fd, &iov, 1, 0, &nread, &roflags);
    expect_errno("not socket", err, __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_recv_file") == 0);
}

static void test_invalid_iovec_ptr(void)
{
    // From LTP recv01: EFAULT on invalid recv buffer.
    printf("Test 3: invalid iovec pointer\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    const __wasi_iovec_t *bad_iov =
        (const __wasi_iovec_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    err = __wasi_sock_recv(fd, bad_iov, 1, 0, &nread, &roflags);
    expect_errno("invalid iovec", err, __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

static void test_basic_recv(void)
{
    // From LLVM libc send_recv_test.cpp: send + recv over connected sockets.
    printf("Test 4: basic recv\n");
    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    const char msg[] = "hello";
    send_all(client_fd, msg, sizeof(msg) - 1);

    char buf[8] = {0};
    __wasi_iovec_t iov = {.buf = (uint8_t *)buf, .buf_len = sizeof(buf)};
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_errno_t err =
        __wasi_sock_recv(accepted_fd, &iov, 1, 0, &nread, &roflags);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nread == sizeof(msg) - 1);
    assert(memcmp(buf, msg, nread) == 0);
    assert(roflags == 0);

    close_pair(client_fd, server_fd, accepted_fd);
}

static void test_peek_preserves_data(void)
{
    // From gVisor recv tests: MSG_PEEK should not consume data.
    printf("Test 5: peek preserves data\n");
    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    const char msg[] = "peek";
    send_all(client_fd, msg, sizeof(msg) - 1);

    char buf[8] = {0};
    __wasi_iovec_t iov = {.buf = (uint8_t *)buf, .buf_len = sizeof(msg) - 1};
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_errno_t err =
        __wasi_sock_recv(accepted_fd, &iov, 1, __WASI_RIFLAGS_RECV_PEEK, &nread,
                         &roflags);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nread == sizeof(msg) - 1);
    assert(memcmp(buf, msg, nread) == 0);

    memset(buf, 0, sizeof(buf));
    nread = 0;
    err = __wasi_sock_recv(accepted_fd, &iov, 1, 0, &nread, &roflags);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nread == sizeof(msg) - 1);
    assert(memcmp(buf, msg, nread) == 0);

    close_pair(client_fd, server_fd, accepted_fd);
}

static void test_nonblocking_empty(void)
{
    // From LTP send02: recv(MSG_DONTWAIT) on empty socket -> EAGAIN.
    printf("Test 6: nonblocking empty recv -> AGAIN\n");
    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    uint8_t buf[1] = {0};
    __wasi_iovec_t iov = {.buf = buf, .buf_len = sizeof(buf)};
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_errno_t err =
        __wasi_sock_recv(accepted_fd, &iov, 1,
                         __WASI_SOCK_RECV_INPUT_DONT_WAIT, &nread, &roflags);
    expect_errno("nonblocking empty", err, __WASI_ERRNO_AGAIN);

    close_pair(client_fd, server_fd, accepted_fd);
}

static void test_multi_iovec_recv(void)
{
    // From wasmtime file read/write tests: multi-iovec recv.
    printf("Test 7: multi-iovec recv\n");
    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    const char msg[] = "abcdef";
    send_all(client_fd, msg, sizeof(msg) - 1);

    char buf[6] = {0};
    __wasi_iovec_t iov[2];
    iov[0].buf = (uint8_t *)&buf[0];
    iov[0].buf_len = 3;
    iov[1].buf = (uint8_t *)&buf[3];
    iov[1].buf_len = 3;

    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_errno_t err =
        __wasi_sock_recv(accepted_fd, iov, 2, 0, &nread, &roflags);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nread == sizeof(msg) - 1);
    assert(memcmp(buf, msg, nread) == 0);

    close_pair(client_fd, server_fd, accepted_fd);
}

int main(void)
{
    printf("WASIX sock_recv integration tests\n");
    test_invalid_fd();
    test_not_socket();
    test_invalid_iovec_ptr();
    test_basic_recv();
    test_peek_preserves_data();
    test_nonblocking_empty();
    test_multi_iovec_recv();
    if (failures != 0) {
        fprintf(stderr, "%d sock_recv check(s) failed\n", failures);
        assert(0);
    }
    printf("All tests passed!\n");
    return 0;
}
