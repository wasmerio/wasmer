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

static void open_listening_tcp(__wasi_fd_t *server_fd, uint16_t *port)
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

    *port = port_from_addr_be(&local_addr);
    assert(*port != 0);
}

static void connect_client(uint16_t port, __wasi_fd_t *client_fd,
                           __wasi_addr_port_t *client_local)
{
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          client_fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t connect_addr;
    set_ipv4_addr_port_le(&connect_addr, port, 127, 0, 0, 1);
    err = __wasi_sock_connect(*client_fd, &connect_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_addr_local(*client_fd, client_local);
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_invalid_fd(void)
{
    // From LTP accept01: EBADF on invalid fd.
    printf("Test 1: invalid fd\n");
    __wasi_fd_t out_fd = 0;
    __wasi_addr_port_t addr;
    __wasi_errno_t err = __wasi_sock_accept_v2(9999, 0, &out_fd, &addr);
    expect_errno("invalid fd", err, __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    // From LTP accept03: ENOTSOCK on non-socket fd.
    printf("Test 2: not a socket\n");
    int fd = open("sock_accept_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_fd_t out_fd = 0;
    __wasi_addr_port_t addr;
    __wasi_errno_t err =
        __wasi_sock_accept_v2((__wasi_fd_t)fd, 0, &out_fd, &addr);
    expect_errno("not socket", err, __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_accept_file") == 0);
}

static void test_udp_accept(void)
{
    // From LTP accept01: accept on UDP is not supported.
    printf("Test 3: UDP accept not supported\n");
    __wasi_fd_t udp_fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &udp_fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t bind_addr;
    set_ipv4_addr_port_le(&bind_addr, 0, 127, 0, 0, 1);
    err = __wasi_sock_bind(udp_fd, &bind_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_fd_t out_fd = 0;
    __wasi_addr_port_t addr;
    err = __wasi_sock_accept_v2(udp_fd, 0, &out_fd, &addr);
    expect_errno("udp accept", err, __WASI_ERRNO_NOTSUP);

    close(udp_fd);
}

static void test_nonblocking_no_pending(void)
{
    // From LTP accept01: no queued connections -> EAGAIN when nonblocking.
    printf("Test 4: nonblocking accept with no pending connection\n");
    __wasi_fd_t server_fd = 0;
    uint16_t port = 0;
    open_listening_tcp(&server_fd, &port);

    __wasi_fd_t out_fd = 0;
    __wasi_addr_port_t addr;
    __wasi_errno_t err =
        __wasi_sock_accept_v2(server_fd, __WASI_FDFLAGS_NONBLOCK, &out_fd,
                              &addr);
    expect_errno("nonblocking no pending", err, __WASI_ERRNO_AGAIN);

    close(server_fd);
}

static void test_accept_peer_addr_and_nonblock(void)
{
    // From gVisor accept_bind and LTP accept4_01: accept returns peer addr and honors NONBLOCK.
    printf("Test 5: accept returns peer addr and sets NONBLOCK\n");
    __wasi_fd_t server_fd = 0;
    uint16_t port = 0;
    open_listening_tcp(&server_fd, &port);

    __wasi_fd_t client_fd = 0;
    __wasi_addr_port_t client_local;
    connect_client(port, &client_fd, &client_local);

    __wasi_fd_t accepted_fd = 0;
    __wasi_addr_port_t peer_addr;
    __wasi_errno_t err =
        __wasi_sock_accept_v2(server_fd, __WASI_FDFLAGS_NONBLOCK, &accepted_fd,
                              &peer_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    assert(peer_addr.tag == __WASI_ADDRESS_FAMILY_INET4);
    assert(peer_addr.u.inet4.port == client_local.u.inet4.port);
    assert(peer_addr.u.inet4.addr.n0 == 127);
    assert(peer_addr.u.inet4.addr.n1 == 0);
    assert(peer_addr.u.inet4.addr.h0 == 0);
    assert(peer_addr.u.inet4.addr.h1 == 1);

    __wasi_fdstat_t stat;
    err = __wasi_fd_fdstat_get(accepted_fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert((stat.fs_flags & __WASI_FDFLAGS_NONBLOCK) != 0);

    close(accepted_fd);
    close(client_fd);
    close(server_fd);
}

static void test_invalid_ptrs(void)
{
    // From LTP accept01: invalid sockaddr/ptr -> EFAULT.
    printf("Test 6: invalid output pointers\n");
    __wasi_fd_t server_fd = 0;
    uint16_t port = 0;
    open_listening_tcp(&server_fd, &port);

    __wasi_fd_t client_fd = 0;
    __wasi_addr_port_t client_local;
    connect_client(port, &client_fd, &client_local);

    __wasi_addr_port_t addr;
    __wasi_fd_t *bad_fd = (__wasi_fd_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_addr_port_t *bad_addr =
        (__wasi_addr_port_t *)(uintptr_t)0xFFFFFFFFu;

    __wasi_errno_t err = __wasi_sock_accept_v2(server_fd, 0, bad_fd, &addr);
    expect_errno("invalid ro_fd", err, __WASI_ERRNO_MEMVIOLATION);

    err = __wasi_sock_accept_v2(server_fd, 0, &server_fd, bad_addr);
    expect_errno("invalid ro_addr", err, __WASI_ERRNO_MEMVIOLATION);

    close(client_fd);
    close(server_fd);
}

int main(void)
{
    printf("WASIX sock_accept integration tests\n");

    test_invalid_fd();
    test_not_socket();
    test_udp_accept();
    test_nonblocking_no_pending();
    test_accept_peer_addr_and_nonblock();
    test_invalid_ptrs();

    if (failures) {
        fprintf(stderr, "%d test(s) failed\n", failures);
        return 1;
    }

    printf("All tests passed!\n");
    return 0;
}
