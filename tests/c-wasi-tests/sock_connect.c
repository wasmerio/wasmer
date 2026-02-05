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

static void test_invalid_fd(void)
{
    printf("Test 1: invalid fd\n");
    __wasi_addr_port_t addr;
    set_ipv4_addr_port_le(&addr, 80, 127, 0, 0, 1);
    __wasi_errno_t err = __wasi_sock_connect(9999, &addr);
    expect_errno("invalid fd", err, __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    printf("Test 2: not a socket\n");
    int fd = open("sock_connect_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_addr_port_t addr;
    set_ipv4_addr_port_le(&addr, 80, 127, 0, 0, 1);
    __wasi_errno_t err = __wasi_sock_connect((__wasi_fd_t)fd, &addr);
    expect_errno("not socket", err, __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_connect_file") == 0);
}

static void test_invalid_addr_ptr(void)
{
    printf("Test 3: invalid address pointer\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t *bad_addr =
        (__wasi_addr_port_t *)(uintptr_t)0xFFFFFFFFu;
    err = __wasi_sock_connect(fd, bad_addr);
    expect_errno("invalid addr", err, __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

static void test_invalid_family(void)
{
    printf("Test 4: invalid address family\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t addr;
    memset(&addr, 0, sizeof(addr));
    addr.tag = 99;
    err = __wasi_sock_connect(fd, &addr);
    expect_errno("invalid family", err, __WASI_ERRNO_INVAL);

    close(fd);
}

static void test_connection_refused(void)
{
    printf("Test 5: connection refused\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t bind_addr;
    set_ipv4_addr_port_le(&bind_addr, 0, 127, 0, 0, 1);
    err = __wasi_sock_bind(fd, &bind_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t local_addr;
    err = __wasi_sock_addr_local(fd, &local_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint16_t port = port_from_addr_be(&local_addr);
    close(fd);

    __wasi_fd_t client = 0;
    err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                           __WASI_SOCK_TYPE_SOCKET_STREAM,
                           __WASI_SOCK_PROTO_TCP,
                           &client);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t target;
    set_ipv4_addr_port_le(&target, port, 127, 0, 0, 1);
    err = __wasi_sock_connect(client, &target);
    expect_errno("connrefused", err, __WASI_ERRNO_CONNREFUSED);

    close(client);
}

static void test_already_connected(void)
{
    printf("Test 6: already connected\n");
    __wasi_fd_t server_fd = 0;
    uint16_t port = 0;
    open_listening_tcp(&server_fd, &port);

    __wasi_fd_t client = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &client);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t target;
    set_ipv4_addr_port_le(&target, port, 127, 0, 0, 1);
    err = __wasi_sock_connect(client, &target);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_connect(client, &target);
    expect_errno("already connected", err, __WASI_ERRNO_ISCONN);

    close(client);
    close(server_fd);
}

static void test_connect_success_and_peer(void)
{
    printf("Test 7: connect success and peer address\n");
    __wasi_fd_t server_fd = 0;
    uint16_t port = 0;
    open_listening_tcp(&server_fd, &port);

    __wasi_fd_t client = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &client);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t target;
    set_ipv4_addr_port_le(&target, port, 127, 0, 0, 1);
    err = __wasi_sock_connect(client, &target);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t peer;
    err = __wasi_sock_addr_peer(client, &peer);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(peer.tag == __WASI_ADDRESS_FAMILY_INET4);
    assert(peer.u.inet4.addr.n0 == 127);
    assert(peer.u.inet4.addr.n1 == 0);
    assert(peer.u.inet4.addr.h0 == 0);
    assert(peer.u.inet4.addr.h1 == 1);
    assert(port_from_addr_be(&peer) == port);

    close(client);
    close(server_fd);
}

static void test_udp_connect_sets_peer(void)
{
    printf("Test 8: UDP connect sets peer address\n");
    __wasi_fd_t sock = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &sock);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t target;
    set_ipv4_addr_port_le(&target, 12345, 127, 0, 0, 1);
    err = __wasi_sock_connect(sock, &target);
    if (err != __WASI_ERRNO_SUCCESS) {
        expect_errno("udp connect", err, __WASI_ERRNO_SUCCESS);
        close(sock);
        return;
    }

    __wasi_addr_port_t peer;
    err = __wasi_sock_addr_peer(sock, &peer);
    if (err != __WASI_ERRNO_SUCCESS) {
        expect_errno("udp peer", err, __WASI_ERRNO_SUCCESS);
        close(sock);
        return;
    }
    assert(peer.tag == __WASI_ADDRESS_FAMILY_INET4);
    assert(peer.u.inet4.addr.n0 == 127);
    assert(peer.u.inet4.addr.n1 == 0);
    assert(peer.u.inet4.addr.h0 == 0);
    assert(peer.u.inet4.addr.h1 == 1);
    assert(port_from_addr_be(&peer) == 12345);

    close(sock);
}

int main(void)
{
    printf("WASIX sock_connect integration tests\n");

    test_invalid_fd();
    test_not_socket();
    test_invalid_addr_ptr();
    test_invalid_family();
    test_connection_refused();
    test_already_connected();
    test_connect_success_and_peer();
    test_udp_connect_sets_peer();

    if (failures) {
        fprintf(stderr, "%d test(s) failed\n", failures);
        return 1;
    }

    printf("All tests passed!\n");
    return 0;
}
