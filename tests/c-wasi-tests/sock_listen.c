#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

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

static void test_invalid_fd(void)
{
    // From LTP listen01: EBADF on invalid fd.
    printf("Test 1: invalid fd\n");
    __wasi_errno_t err = __wasi_sock_listen(9999, 1);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    // From LTP listen01: ENOTSOCK on non-socket fd.
    printf("Test 2: not a socket\n");
    int fd = open("sock_listen_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_sock_listen((__wasi_fd_t)fd, 1);
    assert(err == __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_listen_file") == 0);
}

static void test_udp_not_supported(void)
{
    // From LTP listen01: EOPNOTSUPP on UDP listen.
    printf("Test 3: UDP listen not supported\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_listen(fd, 1);
    assert(err == __WASI_ERRNO_NOTSUP);

    close(fd);
}

static void test_listen_success(void)
{
    // From libc-test socket.c: listen succeeds after bind.
    printf("Test 4: listen success after bind\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t addr;
    set_ipv4_addr_port_le(&addr, 0, 127, 0, 0, 1);
    err = __wasi_sock_bind(fd, &addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_listen(fd, 1);
    assert(err == __WASI_ERRNO_SUCCESS);

    close(fd);
}

int main(void)
{
    test_invalid_fd();
    test_not_socket();
    test_udp_not_supported();
    test_listen_success();
    printf("All tests passed!\n");
    return 0;
}
