#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

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

static void open_connected_udp(__wasi_fd_t *client_fd, __wasi_fd_t *server_fd)
{
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          server_fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t bind_addr;
    set_ipv4_addr_port_le(&bind_addr, 0, 127, 0, 0, 1);
    err = __wasi_sock_bind(*server_fd, &bind_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t local_addr;
    err = __wasi_sock_addr_local(*server_fd, &local_addr);
    assert(err == __WASI_ERRNO_SUCCESS);
    uint16_t port = port_from_addr_be(&local_addr);
    assert(port != 0);

    err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                           __WASI_SOCK_TYPE_SOCKET_DGRAM,
                           __WASI_SOCK_PROTO_UDP,
                           client_fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t connect_addr;
    set_ipv4_addr_port_le(&connect_addr, port, 127, 0, 0, 1);
    err = __wasi_sock_connect(*client_fd, &connect_addr);
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

static void recv_exact(__wasi_fd_t fd, void *out, size_t len)
{
    uint8_t *bytes = (uint8_t *)out;
    size_t offset = 0;
    while (offset < len) {
        __wasi_iovec_t iov = {
            .buf = bytes + offset,
            .buf_len = len - offset,
        };
        __wasi_size_t nread = 0;
        __wasi_roflags_t roflags = 0;
        __wasi_errno_t err = __wasi_sock_recv(fd, &iov, 1, 0, &nread, &roflags);
        assert(err == __WASI_ERRNO_SUCCESS);
        assert(nread > 0);
        offset += nread;
    }
}

static void test_invalid_fd(void)
{
    // From LTP send01: EBADF on invalid fd.
    printf("Test 1: invalid fd\n");
    const char msg[] = "x";
    __wasi_ciovec_t iov = {.buf = (const uint8_t *)msg, .buf_len = 1};
    __wasi_size_t nsent = 0;
    __wasi_errno_t err = __wasi_sock_send(9999, &iov, 1, 0, &nsent);
    expect_errno("invalid fd", err, __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    // From LTP send01: ENOTSOCK on non-socket fd.
    printf("Test 2: not a socket\n");
    int fd = open("sock_send_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    const char msg[] = "x";
    __wasi_ciovec_t iov = {.buf = (const uint8_t *)msg, .buf_len = 1};
    __wasi_size_t nsent = 0;
    __wasi_errno_t err =
        __wasi_sock_send((__wasi_fd_t)fd, &iov, 1, 0, &nsent);
    expect_errno("not socket", err, __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_send_file") == 0);
}

static void test_invalid_iovec_ptr(void)
{
    // From LTP send01: EFAULT on invalid iovec pointer.
    printf("Test 3: invalid iovec pointer\n");
    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    const __wasi_ciovec_t *bad_iov =
        (const __wasi_ciovec_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_size_t nsent = 0;
    __wasi_errno_t err = __wasi_sock_send(client_fd, bad_iov, 1, 0, &nsent);
    expect_errno("invalid iovec", err, __WASI_ERRNO_MEMVIOLATION);

    close_pair(client_fd, server_fd, accepted_fd);
}

static void test_invalid_buffer(void)
{
    // From LTP send01: EFAULT on invalid send buffer.
    printf("Test 4: invalid buffer\n");
    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    __wasi_ciovec_t iov;
    iov.buf = (const uint8_t *)0xFFFFF000u;
    iov.buf_len = 4;
    __wasi_size_t nsent = 0;
    __wasi_errno_t err = __wasi_sock_send(client_fd, &iov, 1, 0, &nsent);
    expect_errno("invalid buffer", err, __WASI_ERRNO_MEMVIOLATION);

    close_pair(client_fd, server_fd, accepted_fd);
}

static void test_basic_send(void)
{
    // From LLVM libc send_recv_test.cpp: send succeeds with socket pair.
    printf("Test 5: basic send\n");
    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    const char msg[] = "hello";
    __wasi_ciovec_t iov = {.buf = (const uint8_t *)msg, .buf_len = 5};
    __wasi_size_t nsent = 0;
    __wasi_errno_t err = __wasi_sock_send(client_fd, &iov, 1, 0, &nsent);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nsent == 5);

    char buf[8] = {0};
    recv_exact(accepted_fd, buf, 5);
    assert(memcmp(buf, msg, 5) == 0);

    close_pair(client_fd, server_fd, accepted_fd);
}

static void test_multi_iovec_send(void)
{
    // From wasmtime read/write tests: multi-iovec send.
    printf("Test 6: multi-iovec send\n");
    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    const char a[] = "ab";
    const char b[] = "cd";
    __wasi_ciovec_t iov[2];
    iov[0].buf = (const uint8_t *)a;
    iov[0].buf_len = 2;
    iov[1].buf = (const uint8_t *)b;
    iov[1].buf_len = 2;

    __wasi_size_t nsent = 0;
    __wasi_errno_t err = __wasi_sock_send(client_fd, iov, 2, 0, &nsent);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nsent == 4);

    char buf[4] = {0};
    recv_exact(accepted_fd, buf, 4);
    assert(memcmp(buf, "abcd", 4) == 0);

    close_pair(client_fd, server_fd, accepted_fd);
}

static void test_zero_length_send(void)
{
    // From POSIX send(): send with length 0 returns 0.
    printf("Test 7: zero-length send\n");
    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    const char msg[] = "x";
    __wasi_ciovec_t iov = {.buf = (const uint8_t *)msg, .buf_len = 0};
    __wasi_size_t nsent = 123;
    __wasi_errno_t err = __wasi_sock_send(client_fd, &iov, 1, 0, &nsent);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nsent == 0);

    close_pair(client_fd, server_fd, accepted_fd);
}

static void test_udp_message_too_big(void)
{
    // From LTP send01: UDP message too big -> EMSGSIZE.
    printf("Test 8: UDP message too big\n");
    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    open_connected_udp(&client_fd, &server_fd);

    const char tiny[] = "x";
    __wasi_ciovec_t small_iov = {.buf = (const uint8_t *)tiny, .buf_len = 1};
    __wasi_size_t nsent = 0;
    __wasi_errno_t err = __wasi_sock_send(client_fd, &small_iov, 1, 0, &nsent);
    if (err != __WASI_ERRNO_SUCCESS || nsent != 1) {
        fprintf(stderr, "udp small send failed: err=%u nsent=%u\n", err,
                (unsigned)nsent);
        failures++;
        close(client_fd);
        close(server_fd);
        return;
    }

    static uint8_t bigbuf[128 * 1024];
    memset(bigbuf, 0x42, sizeof(bigbuf));

    __wasi_ciovec_t iov = {.buf = bigbuf, .buf_len = sizeof(bigbuf)};
    nsent = 0;
    err = __wasi_sock_send(client_fd, &iov, 1, 0, &nsent);
    expect_errno("udp msg too big", err, __WASI_ERRNO_MSGSIZE);

    close(client_fd);
    close(server_fd);
}

static void test_send_after_shutdown(void)
{
    // From LTP send01: local endpoint shutdown -> EPIPE.
    printf("Test 9: send after shutdown\n");
    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    __wasi_errno_t err = __wasi_sock_shutdown(client_fd, __WASI_SDFLAGS_WR);
    assert(err == __WASI_ERRNO_SUCCESS);

    const char msg[] = "x";
    __wasi_ciovec_t iov = {.buf = (const uint8_t *)msg, .buf_len = 1};
    __wasi_size_t nsent = 0;
    err = __wasi_sock_send(client_fd, &iov, 1, 0, &nsent);
    expect_errno("send after shutdown", err, __WASI_ERRNO_PIPE);

    close_pair(client_fd, server_fd, accepted_fd);
}

int main(void)
{
    printf("WASIX sock_send integration tests\n");
    test_invalid_fd();
    test_not_socket();
    test_invalid_iovec_ptr();
    test_invalid_buffer();
    test_basic_send();
    test_multi_iovec_send();
    test_zero_length_send();
    test_udp_message_too_big();
    test_send_after_shutdown();
    if (failures != 0) {
        fprintf(stderr, "%d sock_send check(s) failed\n", failures);
        assert(0);
    }
    printf("All tests passed!\n");
    return 0;
}
