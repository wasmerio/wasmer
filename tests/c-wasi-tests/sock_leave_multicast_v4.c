#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

static void set_ipv4(__wasi_addr_ip4_t *addr, uint8_t a, uint8_t b, uint8_t c,
                     uint8_t d)
{
    memset(addr, 0, sizeof(*addr));
    addr->n0 = a;
    addr->n1 = b;
    addr->h0 = c;
    addr->h1 = d;
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

static void test_invalid_fd(void)
{
    printf("Test 1: invalid fd\n");
    __wasi_addr_ip4_t group;
    __wasi_addr_ip4_t iface;
    set_ipv4(&group, 224, 0, 0, 1);
    set_ipv4(&iface, 0, 0, 0, 0);
    __wasi_errno_t err =
        __wasi_sock_leave_multicast_v4(9999, &group, &iface);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    printf("Test 2: not a socket\n");
    int fd = open("sock_leave_v4_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_addr_ip4_t group;
    __wasi_addr_ip4_t iface;
    set_ipv4(&group, 224, 0, 0, 1);
    set_ipv4(&iface, 0, 0, 0, 0);

    __wasi_errno_t err =
        __wasi_sock_leave_multicast_v4((__wasi_fd_t)fd, &group, &iface);
    assert(err == __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_leave_v4_file") == 0);
}

static void test_invalid_pointer(void)
{
    printf("Test 3: invalid pointer\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_ip4_t *bad_ptr = (__wasi_addr_ip4_t *)(uintptr_t)0xFFFFFFFFu;
    err = __wasi_sock_leave_multicast_v4(fd, bad_ptr, bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

static void test_join_then_leave(void)
{
    printf("Test 4: join then leave\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t bind_addr;
    set_ipv4_addr_port_le(&bind_addr, 0, 0, 0, 0, 0);
    err = __wasi_sock_bind(fd, &bind_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_ip4_t group;
    __wasi_addr_ip4_t iface;
    set_ipv4(&group, 224, 0, 0, 1);
    set_ipv4(&iface, 0, 0, 0, 0);

    err = __wasi_sock_join_multicast_v4(fd, &group, &iface);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_leave_multicast_v4(fd, &group, &iface);
    assert(err == __WASI_ERRNO_SUCCESS);

    close(fd);
}

int main(void)
{
    printf("WASIX sock_leave_multicast_v4 integration tests\n");
    test_invalid_fd();
    test_not_socket();
    test_invalid_pointer();
    test_join_then_leave();
    printf("All tests passed!\n");
    return 0;
}
