#include <assert.h>
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

static void create_connected_pair(__wasi_fd_t *server_fd,
                                  __wasi_fd_t *client_fd,
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

static void test_invalid_fd(void)
{
    // From LTP sctp test_1_to_1_shutdown: EBADF on invalid fd.
    printf("Test 1: invalid fd\n");
    __wasi_errno_t err = __wasi_sock_shutdown(9999, __WASI_SDFLAGS_WR);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    // From LTP sctp test_1_to_1_shutdown and WAMR issue-2787: ENOTSOCK on non-socket fd.
    printf("Test 2: not a socket\n");
    int fd = open("sock_shutdown_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_sock_shutdown((__wasi_fd_t)fd, __WASI_SDFLAGS_WR);
    assert(err == __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_shutdown_file") == 0);
}

static void test_invalid_how(void)
{
    printf("Test 3: invalid shutdown flags\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_shutdown(fd, 0);
    assert(err == __WASI_ERRNO_INVAL);

    err = __wasi_sock_shutdown(fd, (__wasi_sdflags_t)0xFF);
    assert(err == __WASI_ERRNO_INVAL);

    close(fd);
}

static void test_unconnected_socket(void)
{
    // From LTP sctp test_1_to_1_shutdown: ENOTCONN on unconnected socket.
    printf("Test 4: unconnected socket\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_shutdown(fd, __WASI_SDFLAGS_RD);
    assert(err == __WASI_ERRNO_NOTCONN);

    close(fd);
}

static void test_udp_not_supported(void)
{
    // UDP socket shutdown is not supported in WASIX.
    printf("Test 5: UDP shutdown not supported\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t bind_addr;
    set_ipv4_addr_port_le(&bind_addr, 0, 127, 0, 0, 1);
    err = __wasi_sock_bind(fd, &bind_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_shutdown(fd, (__wasi_sdflags_t)(__WASI_SDFLAGS_RD | __WASI_SDFLAGS_WR));
    assert(err == __WASI_ERRNO_NOTSUP);

    close(fd);
}

static void test_listener_not_supported(void)
{
    printf("Test 6: listener shutdown not supported\n");
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
    err = __wasi_sock_listen(fd, 1);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_shutdown(fd, __WASI_SDFLAGS_WR);
    assert(err == __WASI_ERRNO_NOTSUP);

    close(fd);
}

static void test_connected_shutdown_modes(void)
{
    // From LTP sctp test_1_to_1_shutdown: shutdown succeeds for WR/RD/RDWR.
    printf("Test 7: connected shutdown modes\n");

    __wasi_fd_t server_fd = 0, client_fd = 0, accepted_fd = 0;
    create_connected_pair(&server_fd, &client_fd, &accepted_fd);
    __wasi_errno_t err = __wasi_sock_shutdown(client_fd, __WASI_SDFLAGS_WR);
    assert(err == __WASI_ERRNO_SUCCESS);
    close(accepted_fd);
    close(client_fd);
    close(server_fd);

    create_connected_pair(&server_fd, &client_fd, &accepted_fd);
    err = __wasi_sock_shutdown(client_fd, __WASI_SDFLAGS_RD);
    assert(err == __WASI_ERRNO_SUCCESS);
    close(accepted_fd);
    close(client_fd);
    close(server_fd);

    create_connected_pair(&server_fd, &client_fd, &accepted_fd);
    err = __wasi_sock_shutdown(
        client_fd,
        (__wasi_sdflags_t)(__WASI_SDFLAGS_RD | __WASI_SDFLAGS_WR));
    assert(err == __WASI_ERRNO_SUCCESS);
    close(accepted_fd);
    close(client_fd);
    close(server_fd);
}

int main(void)
{
    test_invalid_fd();
    test_not_socket();
    test_invalid_how();
    test_unconnected_socket();
    test_udp_not_supported();
    test_listener_not_supported();
    test_connected_shutdown_modes();
    printf("All tests passed!\n");
    return 0;
}
