#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

static void set_ipv6(__wasi_addr_ip6_t *addr, uint16_t n0, uint16_t n1,
                     uint16_t n2, uint16_t n3, uint16_t h0, uint16_t h1,
                     uint16_t h2, uint16_t h3)
{
    memset(addr, 0, sizeof(*addr));
    addr->n0 = n0;
    addr->n1 = n1;
    addr->n2 = n2;
    addr->n3 = n3;
    addr->h0 = h0;
    addr->h1 = h1;
    addr->h2 = h2;
    addr->h3 = h3;
}

static void set_ipv6_addr_port_le(__wasi_addr_port_t *addr, uint16_t port)
{
    memset(addr, 0, sizeof(*addr));
    addr->tag = __WASI_ADDRESS_FAMILY_INET6;
    unsigned char *octs = (unsigned char *)&addr->u;
    octs[0] = (unsigned char)(port & 0xff);
    octs[1] = (unsigned char)((port >> 8) & 0xff);
    // IPv6 :: (all zeros) already set by memset.
}

static void test_invalid_fd(void)
{
    printf("Test 1: invalid fd\n");
    __wasi_addr_ip6_t group;
    set_ipv6(&group, 0xff02, 0, 0, 0, 0, 0, 0, 1);
    __wasi_errno_t err = __wasi_sock_leave_multicast_v6(9999, &group, 1);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    printf("Test 2: not a socket\n");
    int fd = open("sock_leave_v6_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_addr_ip6_t group;
    set_ipv6(&group, 0xff02, 0, 0, 0, 0, 0, 0, 1);
    __wasi_errno_t err =
        __wasi_sock_leave_multicast_v6((__wasi_fd_t)fd, &group, 1);
    assert(err == __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_leave_v6_file") == 0);
}

static void test_invalid_pointer(void)
{
    printf("Test 3: invalid pointer\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET6,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_ip6_t *bad_ptr = (__wasi_addr_ip6_t *)(uintptr_t)0xFFFFFFFFu;
    err = __wasi_sock_leave_multicast_v6(fd, bad_ptr, 1);
    assert(err == __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

static void test_join_then_leave(void)
{
    printf("Test 4: join then leave\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET6,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t bind_addr;
    set_ipv6_addr_port_le(&bind_addr, 0);
    err = __wasi_sock_bind(fd, &bind_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_ip6_t group;
    set_ipv6(&group, 0xff02, 0, 0, 0, 0, 0, 0, 1);

    // iface=1 is loopback on macOS and commonly on Linux.
    err = __wasi_sock_join_multicast_v6(fd, &group, 1);
    if (err != __WASI_ERRNO_SUCCESS) {
        fprintf(stderr, "join_multicast_v6 failed: %u\n", err);
        assert(0);
    }

    err = __wasi_sock_leave_multicast_v6(fd, &group, 1);
    if (err != __WASI_ERRNO_SUCCESS) {
        fprintf(stderr, "leave_multicast_v6 failed: %u\n", err);
        assert(0);
    }

    close(fd);
}

int main(void)
{
    printf("WASIX sock_leave_multicast_v6 integration tests\n");
    test_invalid_fd();
    test_not_socket();
    test_invalid_pointer();
    test_join_then_leave();
    printf("All tests passed!\n");
    return 0;
}
