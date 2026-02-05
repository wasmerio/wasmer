#include <assert.h>
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

static void set_ipv4_addr(__wasi_addr_ip4_t *addr, uint8_t a, uint8_t b,
                          uint8_t c, uint8_t d)
{
    memset(addr, 0, sizeof(*addr));
    addr->n0 = a;
    addr->n1 = b;
    addr->h0 = c;
    addr->h1 = d;
}

static void set_ipv6_addr(__wasi_addr_ip6_t *addr, const uint8_t octs[16])
{
    memset(addr, 0, sizeof(*addr));
    addr->n0 = ((uint16_t)octs[0] << 8) | octs[1];
    addr->n1 = ((uint16_t)octs[2] << 8) | octs[3];
    addr->n2 = ((uint16_t)octs[4] << 8) | octs[5];
    addr->n3 = ((uint16_t)octs[6] << 8) | octs[7];
    addr->h0 = ((uint16_t)octs[8] << 8) | octs[9];
    addr->h1 = ((uint16_t)octs[10] << 8) | octs[11];
    addr->h2 = ((uint16_t)octs[12] << 8) | octs[13];
    addr->h3 = ((uint16_t)octs[14] << 8) | octs[15];
}

static void test_v4_invalid_fd(void)
{
    printf("Test 1: v4 invalid fd\n");
    __wasi_addr_ip4_t mcast;
    __wasi_addr_ip4_t iface;
    set_ipv4_addr(&mcast, 224, 0, 0, 1);
    set_ipv4_addr(&iface, 0, 0, 0, 0);
    __wasi_errno_t err = __wasi_sock_join_multicast_v4(9999, &mcast, &iface);
    expect_errno("v4 invalid fd", err, __WASI_ERRNO_BADF);
}

static void test_v4_not_socket(void)
{
    printf("Test 2: v4 not a socket\n");
    __wasi_addr_ip4_t mcast;
    __wasi_addr_ip4_t iface;
    set_ipv4_addr(&mcast, 224, 0, 0, 1);
    set_ipv4_addr(&iface, 0, 0, 0, 0);
    __wasi_errno_t err = __wasi_sock_join_multicast_v4(0, &mcast, &iface);
    expect_errno("v4 not socket", err, __WASI_ERRNO_NOTSOCK);
}

static void test_v4_invalid_ptrs(void)
{
    printf("Test 3: v4 invalid pointers\n");
    __wasi_fd_t sock = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &sock);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_ip4_t iface;
    set_ipv4_addr(&iface, 0, 0, 0, 0);
    __wasi_addr_ip4_t *bad_addr = (__wasi_addr_ip4_t *)(uintptr_t)0xFFFFFFFFu;

    err = __wasi_sock_join_multicast_v4(sock, bad_addr, &iface);
    expect_errno("v4 bad multiaddr", err, __WASI_ERRNO_MEMVIOLATION);

    err = __wasi_sock_join_multicast_v4(sock, &iface, bad_addr);
    expect_errno("v4 bad iface", err, __WASI_ERRNO_MEMVIOLATION);

    close(sock);
}

static void test_v4_basic_join(void)
{
    printf("Test 4: v4 basic join\n");
    __wasi_fd_t sock = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &sock);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_ip4_t mcast;
    __wasi_addr_ip4_t iface;
    set_ipv4_addr(&mcast, 224, 0, 0, 1);
    set_ipv4_addr(&iface, 0, 0, 0, 0);

    err = __wasi_sock_join_multicast_v4(sock, &mcast, &iface);
    assert(err == __WASI_ERRNO_SUCCESS);

    close(sock);
}

static void test_v6_invalid_fd(void)
{
    printf("Test 5: v6 invalid fd\n");
    __wasi_addr_ip6_t mcast;
    const uint8_t ff01[16] = {0xff, 0x01, 0, 0, 0, 0, 0, 0,
                              0, 0, 0, 0, 0, 0, 0, 1};
    set_ipv6_addr(&mcast, ff01);
    __wasi_errno_t err = __wasi_sock_join_multicast_v6(9999, &mcast, 0);
    expect_errno("v6 invalid fd", err, __WASI_ERRNO_BADF);
}

static void test_v6_not_socket(void)
{
    printf("Test 6: v6 not a socket\n");
    __wasi_addr_ip6_t mcast;
    const uint8_t ff01[16] = {0xff, 0x01, 0, 0, 0, 0, 0, 0,
                              0, 0, 0, 0, 0, 0, 0, 1};
    set_ipv6_addr(&mcast, ff01);
    __wasi_errno_t err = __wasi_sock_join_multicast_v6(0, &mcast, 0);
    expect_errno("v6 not socket", err, __WASI_ERRNO_NOTSOCK);
}

static void test_v6_invalid_ptr(void)
{
    printf("Test 7: v6 invalid pointer\n");
    __wasi_fd_t sock = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET6,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &sock);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_ip6_t *bad_addr = (__wasi_addr_ip6_t *)(uintptr_t)0xFFFFFFFFu;
    err = __wasi_sock_join_multicast_v6(sock, bad_addr, 0);
    expect_errno("v6 bad multiaddr", err, __WASI_ERRNO_MEMVIOLATION);

    close(sock);
}

static void test_v6_basic_join(void)
{
    printf("Test 8: v6 basic join\n");
    __wasi_fd_t sock = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET6,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &sock);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_ip6_t mcast;
    const uint8_t ff01[16] = {0xff, 0x01, 0, 0, 0, 0, 0, 0,
                              0, 0, 0, 0, 0, 0, 0, 1};
    set_ipv6_addr(&mcast, ff01);

    err = __wasi_sock_join_multicast_v6(sock, &mcast, 0);
    assert(err == __WASI_ERRNO_SUCCESS);

    close(sock);
}

int main(void)
{
    printf("WASIX sock_join_multicast integration tests\n");

    test_v4_invalid_fd();
    test_v4_not_socket();
    test_v4_invalid_ptrs();
    test_v4_basic_join();
    test_v6_invalid_fd();
    test_v6_not_socket();
    test_v6_invalid_ptr();
    test_v6_basic_join();

    if (failures) {
        fprintf(stderr, "%d test(s) failed\n", failures);
        return 1;
    }

    printf("All tests passed!\n");
    return 0;
}
