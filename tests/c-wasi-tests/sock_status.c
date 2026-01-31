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
    printf("Test 1: invalid fd\n");
    __wasi_sock_status_t status = 0;
    __wasi_errno_t err = __wasi_sock_status(9999, &status);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    printf("Test 2: not a socket\n");
    int fd = open("sock_status_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_sock_status_t status = 0;
    __wasi_errno_t err = __wasi_sock_status((__wasi_fd_t)fd, &status);
    assert(err == __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_status_file") == 0);
}

static void test_invalid_pointer(void)
{
    printf("Test 3: invalid pointer\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_sock_status_t *bad_ptr = (__wasi_sock_status_t *)(uintptr_t)0xFFFFFFFFu;
    err = __wasi_sock_status(fd, bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

static void test_opening_status(void)
{
    printf("Test 4: opening status on fresh socket\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_sock_status_t status = 0xFF;
    err = __wasi_sock_status(fd, &status);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(status == __WASI_SOCK_STATUS_OPENING);

    close(fd);
}

static void test_opened_status_listener(void)
{
    printf("Test 5: opened status after listen\n");
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

    __wasi_sock_status_t status = 0xFF;
    err = __wasi_sock_status(fd, &status);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(status == __WASI_SOCK_STATUS_OPENED);

    close(fd);
}

static void test_opened_status_udp(void)
{
    printf("Test 6: opened status after UDP bind\n");
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

    __wasi_sock_status_t status = 0xFF;
    err = __wasi_sock_status(fd, &status);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(status == __WASI_SOCK_STATUS_OPENED);

    close(fd);
}

int main(void)
{
    test_invalid_fd();
    test_not_socket();
    test_invalid_pointer();
    test_opening_status();
    test_opened_status_listener();
    test_opened_status_udp();
    printf("All tests passed!\n");
    return 0;
}
