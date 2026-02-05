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

static void ipv4_from_addr(const __wasi_addr_port_t *addr, uint8_t out[4])
{
    const unsigned char *octs = (const unsigned char *)&addr->u;
    out[0] = octs[2];
    out[1] = octs[3];
    out[2] = octs[4];
    out[3] = octs[5];
}

static void open_udp_bound(__wasi_fd_t *fd, __wasi_addr_port_t *local)
{
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t bind_addr;
    set_ipv4_addr_port_le(&bind_addr, 0, 127, 0, 0, 1);
    err = __wasi_sock_bind(*fd, &bind_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_addr_local(*fd, local);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(local->tag == __WASI_ADDRESS_FAMILY_INET4);
    assert(port_from_addr_be(local) != 0);
}

static void open_connected_udp(__wasi_fd_t *client_fd, __wasi_fd_t *server_fd,
                               __wasi_addr_port_t *client_addr)
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

    __wasi_addr_port_t server_local;
    err = __wasi_sock_addr_local(*server_fd, &server_local);
    assert(err == __WASI_ERRNO_SUCCESS);
    uint16_t port = port_from_addr_be(&server_local);
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

    err = __wasi_sock_addr_local(*client_fd, client_addr);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(client_addr->tag == __WASI_ADDRESS_FAMILY_INET4);
}

static __wasi_errno_t recv_from_retry(__wasi_fd_t fd, __wasi_iovec_t *iov,
                                      __wasi_size_t iov_len,
                                      __wasi_riflags_t flags,
                                      __wasi_size_t *nread,
                                      __wasi_roflags_t *roflags,
                                      __wasi_addr_port_t *peer)
{
    for (int i = 0; i < 1000; i++) {
        __wasi_errno_t err =
            __wasi_sock_recv_from(fd, iov, iov_len,
                                  flags | __WASI_SOCK_RECV_INPUT_DONT_WAIT,
                                  nread, roflags, peer);
        if (err == __WASI_ERRNO_AGAIN) {
            continue;
        }
        return err;
    }
    return __WASI_ERRNO_AGAIN;
}

static void test_invalid_fd(void)
{
    printf("Test 1: invalid fd\n");
    uint8_t buf[4] = {0};
    __wasi_iovec_t iov = {.buf = buf, .buf_len = sizeof(buf)};
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_addr_port_t addr;
    __wasi_errno_t err =
        __wasi_sock_recv_from(9999, &iov, 1, 0, &nread, &roflags, &addr);
    expect_errno("invalid fd", err, __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    printf("Test 2: not a socket\n");
    int fd = open("sock_recv_from_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    uint8_t buf[4] = {0};
    __wasi_iovec_t iov = {.buf = buf, .buf_len = sizeof(buf)};
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_addr_port_t addr;
    __wasi_errno_t err =
        __wasi_sock_recv_from((__wasi_fd_t)fd, &iov, 1, 0, &nread, &roflags,
                               &addr);
    expect_errno("not socket", err, __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_recv_from_file") == 0);
}

static void test_invalid_iovec(void)
{
    printf("Test 3: invalid iovec pointer\n");
    __wasi_fd_t fd = 0;
    __wasi_addr_port_t local;
    open_udp_bound(&fd, &local);

    __wasi_iovec_t *bad_iov = (__wasi_iovec_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_addr_port_t addr;

    __wasi_errno_t err =
        __wasi_sock_recv_from(fd, bad_iov, 1, 0, &nread, &roflags, &addr);
    expect_errno("invalid iovec", err, __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

static void test_nonblocking_empty(void)
{
    printf("Test 4: nonblocking empty recv\n");
    __wasi_fd_t fd = 0;
    __wasi_addr_port_t local;
    open_udp_bound(&fd, &local);

    uint8_t buf[4] = {0};
    __wasi_iovec_t iov = {.buf = buf, .buf_len = sizeof(buf)};
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_addr_port_t addr;

    __wasi_errno_t err = __wasi_sock_recv_from(
        fd, &iov, 1, __WASI_SOCK_RECV_INPUT_DONT_WAIT, &nread, &roflags, &addr);
    expect_errno("nonblocking empty", err, __WASI_ERRNO_AGAIN);

    close(fd);
}

static void test_basic_recvfrom(void)
{
    printf("Test 5: basic recvfrom + peer address\n");
    __wasi_fd_t recv_fd = 0, send_fd = 0;
    __wasi_addr_port_t send_addr;
    open_connected_udp(&send_fd, &recv_fd, &send_addr);

    const uint8_t msg[] = "hello world";
    __wasi_ciovec_t siov = {.buf = msg, .buf_len = sizeof(msg)};
    __wasi_size_t sent = 0;
    __wasi_errno_t err = __wasi_sock_send(send_fd, &siov, 1, 0, &sent);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(sent == sizeof(msg));

    uint8_t buf1[6] = {0};
    uint8_t buf2[6] = {0};
    __wasi_iovec_t iov[2] = {
        {.buf = buf1, .buf_len = sizeof(buf1)},
        {.buf = buf2, .buf_len = sizeof(buf2)},
    };
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_addr_port_t peer;

    __wasi_errno_t recv_err =
        recv_from_retry(recv_fd, iov, 2, 0, &nread, &roflags, &peer);
    if (recv_err != __WASI_ERRNO_SUCCESS) {
        fprintf(stderr, "basic recvfrom failed: err=%u\n", recv_err);
        failures++;
        close(recv_fd);
        close(send_fd);
        return;
    }
    assert(nread == sizeof(msg));
    assert(roflags == 0);

    uint8_t out[sizeof(msg)] = {0};
    memcpy(out, buf1, sizeof(buf1));
    memcpy(out + sizeof(buf1), buf2, sizeof(buf2));
    assert(memcmp(out, msg, sizeof(msg)) == 0);

    assert(peer.tag == __WASI_ADDRESS_FAMILY_INET4);
    uint8_t peer_ip[4];
    ipv4_from_addr(&peer, peer_ip);
    assert(peer_ip[0] == 127 && peer_ip[1] == 0 && peer_ip[2] == 0 &&
           peer_ip[3] == 1);
    assert(port_from_addr_be(&peer) == port_from_addr_be(&send_addr));

    close(recv_fd);
    close(send_fd);
}

static void test_invalid_ro_addr(void)
{
    printf("Test 6: invalid ro_addr pointer\n");
    __wasi_fd_t recv_fd = 0, send_fd = 0;
    __wasi_addr_port_t send_addr;
    open_connected_udp(&send_fd, &recv_fd, &send_addr);

    const uint8_t msg[] = "ping";
    __wasi_ciovec_t siov = {.buf = msg, .buf_len = sizeof(msg)};
    __wasi_size_t sent = 0;
    __wasi_errno_t err = __wasi_sock_send(send_fd, &siov, 1, 0, &sent);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(sent == sizeof(msg));

    uint8_t buf[8] = {0};
    __wasi_iovec_t iov = {.buf = buf, .buf_len = sizeof(buf)};
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_addr_port_t *bad_addr = (__wasi_addr_port_t *)(uintptr_t)0xFFFFFFFFu;

    __wasi_errno_t recv_err =
        recv_from_retry(recv_fd, &iov, 1, 0, &nread, &roflags,
                        ( __wasi_addr_port_t *)bad_addr);
    expect_errno("invalid ro_addr", recv_err, __WASI_ERRNO_MEMVIOLATION);

    close(recv_fd);
    close(send_fd);
}

static void test_invalid_ro_flags(void)
{
    printf("Test 7: invalid ro_flags pointer\n");
    __wasi_fd_t recv_fd = 0, send_fd = 0;
    __wasi_addr_port_t send_addr;
    open_connected_udp(&send_fd, &recv_fd, &send_addr);

    const uint8_t msg[] = "pong";
    __wasi_ciovec_t siov = {.buf = msg, .buf_len = sizeof(msg)};
    __wasi_size_t sent = 0;
    __wasi_errno_t err = __wasi_sock_send(send_fd, &siov, 1, 0, &sent);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(sent == sizeof(msg));

    uint8_t buf[8] = {0};
    __wasi_iovec_t iov = {.buf = buf, .buf_len = sizeof(buf)};
    __wasi_size_t nread = 0;
    __wasi_addr_port_t peer;
    __wasi_roflags_t *bad_flags = (__wasi_roflags_t *)(uintptr_t)0xFFFFFFFFu;

    __wasi_errno_t recv_err =
        recv_from_retry(recv_fd, &iov, 1, 0, &nread,
                        (__wasi_roflags_t *)bad_flags, &peer);
    expect_errno("invalid ro_flags", recv_err, __WASI_ERRNO_MEMVIOLATION);

    close(recv_fd);
    close(send_fd);
}

static void test_invalid_ro_data_len(void)
{
    printf("Test 8: invalid ro_data_len pointer\n");
    __wasi_fd_t recv_fd = 0, send_fd = 0;
    __wasi_addr_port_t send_addr;
    open_connected_udp(&send_fd, &recv_fd, &send_addr);

    const uint8_t msg[] = "data";
    __wasi_ciovec_t siov = {.buf = msg, .buf_len = sizeof(msg)};
    __wasi_size_t sent = 0;
    __wasi_errno_t err = __wasi_sock_send(send_fd, &siov, 1, 0, &sent);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(sent == sizeof(msg));

    uint8_t buf[8] = {0};
    __wasi_iovec_t iov = {.buf = buf, .buf_len = sizeof(buf)};
    __wasi_roflags_t roflags = 0;
    __wasi_addr_port_t peer;
    __wasi_size_t *bad_len = (__wasi_size_t *)(uintptr_t)0xFFFFFFFFu;

    __wasi_errno_t recv_err =
        recv_from_retry(recv_fd, &iov, 1, 0, (__wasi_size_t *)bad_len, &roflags,
                        &peer);
    expect_errno("invalid ro_data_len", recv_err, __WASI_ERRNO_MEMVIOLATION);

    close(recv_fd);
    close(send_fd);
}

int main(void)
{
    printf("WASIX sock_recv_from integration tests\n");

    test_invalid_fd();
    test_not_socket();
    test_invalid_iovec();
    test_nonblocking_empty();
    test_basic_recvfrom();
    test_invalid_ro_addr();
    test_invalid_ro_flags();
    test_invalid_ro_data_len();

    if (failures) {
        fprintf(stderr, "%d test(s) failed\n", failures);
        return 1;
    }

    printf("All tests passed!\n");
    return 0;
}
