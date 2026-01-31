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

static __wasi_fd_t open_udp_socket(void)
{
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(
        __WASI_ADDRESS_FAMILY_INET4,
        __WASI_SOCK_TYPE_SOCKET_DGRAM,
        __WASI_SOCK_PROTO_UDP,
        &fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    return fd;
}

static void test_invalid_fd(void)
{
    printf("Test 1: invalid fd\n");
    __wasi_filesize_t size = 0;
    __wasi_errno_t err = __wasi_sock_get_opt_size(
        9999,
        __WASI_SOCK_OPTION_RECV_BUF_SIZE,
        &size);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    printf("Test 2: not a socket\n");
    int fd = open("sock_get_opt_size_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_filesize_t size = 0;
    __wasi_errno_t err = __wasi_sock_get_opt_size(
        (__wasi_fd_t)fd,
        __WASI_SOCK_OPTION_RECV_BUF_SIZE,
        &size);
    assert(err == __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_get_opt_size_file") == 0);
}

static void test_defaults_and_set_get(void)
{
    printf("Test 3: default sizes and set/get\n");
    __wasi_fd_t fd = open_tcp_socket();

    __wasi_filesize_t size = 1234;
    __wasi_errno_t err = __wasi_sock_get_opt_size(
        fd,
        __WASI_SOCK_OPTION_RECV_BUF_SIZE,
        &size);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(size == 0);

    size = 5678;
    err = __wasi_sock_get_opt_size(
        fd,
        __WASI_SOCK_OPTION_SEND_BUF_SIZE,
        &size);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(size == 0);

    err = __wasi_sock_set_opt_size(
        fd,
        __WASI_SOCK_OPTION_RECV_BUF_SIZE,
        8192);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_set_opt_size(
        fd,
        __WASI_SOCK_OPTION_SEND_BUF_SIZE,
        16384);
    assert(err == __WASI_ERRNO_SUCCESS);

    size = 0;
    err = __wasi_sock_get_opt_size(
        fd,
        __WASI_SOCK_OPTION_RECV_BUF_SIZE,
        &size);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(size == 8192);

    size = 0;
    err = __wasi_sock_get_opt_size(
        fd,
        __WASI_SOCK_OPTION_SEND_BUF_SIZE,
        &size);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(size == 16384);

    close(fd);
}

static void test_udp_ttl(void)
{
    printf("Test 4: TTL and multicast TTL on UDP\n");
    __wasi_fd_t fd = open_udp_socket();

    __wasi_addr_port_t bind_addr;
    set_ipv4_addr_port_le(&bind_addr, 0, 127, 0, 0, 1);
    __wasi_errno_t err = __wasi_sock_bind(fd, &bind_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_set_opt_size(fd, __WASI_SOCK_OPTION_TTL, 42);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filesize_t size = 0;
    err = __wasi_sock_get_opt_size(fd, __WASI_SOCK_OPTION_TTL, &size);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(size == 42);

    err = __wasi_sock_set_opt_size(fd, __WASI_SOCK_OPTION_MULTICAST_TTL_V4, 7);
    assert(err == __WASI_ERRNO_SUCCESS);

    size = 0;
    err = __wasi_sock_get_opt_size(fd, __WASI_SOCK_OPTION_MULTICAST_TTL_V4, &size);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(size == 7);

    close(fd);
}

static void test_invalid_option(void)
{
    printf("Test 5: invalid option\n");
    __wasi_fd_t fd = open_tcp_socket();
    __wasi_filesize_t size = 0;

    __wasi_sock_option_t bad_opt = (__wasi_sock_option_t)0xFFu;
    __wasi_errno_t err = __wasi_sock_get_opt_size(fd, bad_opt, &size);
    assert(err == __WASI_ERRNO_INVAL);

    err = __wasi_sock_get_opt_size(
        fd,
        __WASI_SOCK_OPTION_NO_DELAY,
        &size);
    assert(err == __WASI_ERRNO_INVAL);

    close(fd);
}

static void test_invalid_pointer(void)
{
    printf("Test 6: invalid pointer\n");
    __wasi_fd_t fd = open_tcp_socket();

    __wasi_filesize_t *bad_ptr = (__wasi_filesize_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_sock_get_opt_size(
        fd,
        __WASI_SOCK_OPTION_RECV_BUF_SIZE,
        bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

int main(void)
{
    test_invalid_fd();
    test_not_socket();
    test_defaults_and_set_get();
    test_udp_ttl();
    test_invalid_option();
    test_invalid_pointer();
    printf("All tests passed!\n");
    return 0;
}
