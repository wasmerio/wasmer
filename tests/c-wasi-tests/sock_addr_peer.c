#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api_wasi.h>
#include <wasi/api_wasix.h>

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

static void test_invalid_fd(void)
{
    printf("Test 1: invalid fd\n");
    __wasi_addr_port_t addr;
    __wasi_errno_t ret = __wasi_sock_addr_peer(9999, &addr);
    assert(ret == __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    printf("Test 2: not a socket\n");
    int fd = open("sock_addr_peer_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_addr_port_t addr;
    __wasi_errno_t ret = __wasi_sock_addr_peer((__wasi_fd_t)fd, &addr);
    assert(ret == __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_addr_peer_file") == 0);
}

static void test_connected_peer(void)
{
    printf("Test 3: connected socket peer address\n");
    __wasi_fd_t server_fd = 0;
    __wasi_errno_t ret =
        __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                         __WASI_SOCK_TYPE_SOCKET_STREAM,
                         __WASI_SOCK_PROTO_TCP,
                         &server_fd);
    assert(ret == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t bind_addr;
    set_ipv4_addr_port_le(&bind_addr, 0, 127, 0, 0, 1);
    ret = __wasi_sock_bind(server_fd, &bind_addr);
    assert(ret == __WASI_ERRNO_SUCCESS);
    ret = __wasi_sock_listen(server_fd, 1);
    assert(ret == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t local_addr;
    ret = __wasi_sock_addr_local(server_fd, &local_addr);
    assert(ret == __WASI_ERRNO_SUCCESS);
    assert(local_addr.tag == __WASI_ADDRESS_FAMILY_INET4);
    uint16_t server_port = port_from_addr_be(&local_addr);
    assert(server_port != 0);

    __wasi_fd_t client_fd = 0;
    ret = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                           __WASI_SOCK_TYPE_SOCKET_STREAM,
                           __WASI_SOCK_PROTO_TCP,
                           &client_fd);
    assert(ret == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t connect_addr;
    set_ipv4_addr_port_le(&connect_addr, server_port, 127, 0, 0, 1);
    ret = __wasi_sock_connect(client_fd, &connect_addr);
    assert(ret == __WASI_ERRNO_SUCCESS);

    __wasi_fd_t accepted_fd = 0;
    __wasi_addr_port_t accepted_addr;
    ret = __wasi_sock_accept_v2(server_fd, 0, &accepted_fd, &accepted_addr);
    assert(ret == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t peer_addr;
    ret = __wasi_sock_addr_peer(client_fd, &peer_addr);
    assert(ret == __WASI_ERRNO_SUCCESS);
    assert(peer_addr.tag == __WASI_ADDRESS_FAMILY_INET4);
    uint16_t peer_port = port_from_addr_be(&peer_addr);
    assert(peer_port == server_port);
    uint8_t peer_ip[4];
    ipv4_from_addr(&peer_addr, peer_ip);
    assert(peer_ip[0] == 127 && peer_ip[1] == 0 && peer_ip[2] == 0 &&
           peer_ip[3] == 1);

    __wasi_addr_port_t client_local;
    ret = __wasi_sock_addr_local(client_fd, &client_local);
    assert(ret == __WASI_ERRNO_SUCCESS);
    uint16_t client_port = port_from_addr_be(&client_local);
    assert(client_port != 0);

    __wasi_addr_port_t accepted_peer;
    ret = __wasi_sock_addr_peer(accepted_fd, &accepted_peer);
    assert(ret == __WASI_ERRNO_SUCCESS);
    assert(accepted_peer.tag == __WASI_ADDRESS_FAMILY_INET4);
    uint16_t accepted_peer_port = port_from_addr_be(&accepted_peer);
    assert(accepted_peer_port == client_port);
    uint8_t accepted_peer_ip[4];
    ipv4_from_addr(&accepted_peer, accepted_peer_ip);
    assert(accepted_peer_ip[0] == 127 && accepted_peer_ip[1] == 0 &&
           accepted_peer_ip[2] == 0 && accepted_peer_ip[3] == 1);

    ret = __wasi_sock_addr_peer(client_fd, (__wasi_addr_port_t *)0xFFFFFFFF);
    assert(ret == __WASI_ERRNO_MEMVIOLATION);

    close(accepted_fd);
    close(client_fd);
    close(server_fd);
}

static void test_unconnected_socket(void)
{
    // From LTP getpeername01.c: ENOTCONN on socket not connected.
    printf("Test 4: unconnected socket\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t ret =
        __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                         __WASI_SOCK_TYPE_SOCKET_STREAM,
                         __WASI_SOCK_PROTO_TCP,
                         &fd);
    assert(ret == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t addr;
    ret = __wasi_sock_addr_peer(fd, &addr);
    assert(ret == __WASI_ERRNO_NOTCONN);

    close(fd);
}

int main(void)
{
    test_invalid_fd();
    test_not_socket();
    test_connected_peer();
    test_unconnected_socket();
    printf("All tests passed!\n");
    return 0;
}
