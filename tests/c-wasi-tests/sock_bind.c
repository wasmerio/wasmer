#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api.h>
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

static void set_ipv6_addr_port_le(__wasi_addr_port_t *addr, uint16_t port,
                                  const uint8_t ip[16])
{
    memset(addr, 0, sizeof(*addr));
    addr->tag = __WASI_ADDRESS_FAMILY_INET6;
    unsigned char *octs = (unsigned char *)&addr->u;
    octs[0] = (unsigned char)(port & 0xff);
    octs[1] = (unsigned char)((port >> 8) & 0xff);
    for (int i = 0; i < 16; i++) {
        octs[2 + i] = ip[i];
    }
}

static void test_invalid_fd(void)
{
    // From LTP bind01: EBADF on invalid fd.
    printf("Test 1: invalid fd\n");
    __wasi_addr_port_t addr;
    set_ipv4_addr_port_le(&addr, 0, 127, 0, 0, 1);
    __wasi_errno_t err = __wasi_sock_bind(9999, &addr);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    // From LTP bind01: ENOTSOCK on non-socket fd.
    printf("Test 2: not a socket\n");
    int fd = open("sock_bind_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_addr_port_t addr;
    set_ipv4_addr_port_le(&addr, 0, 127, 0, 0, 1);
    __wasi_errno_t err = __wasi_sock_bind((__wasi_fd_t)fd, &addr);
    assert(err == __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_bind_file") == 0);
}

static void test_invalid_pointer(void)
{
    // From LTP bind01: EFAULT on invalid sockaddr pointer.
    printf("Test 3: invalid address pointer\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t *bad_ptr = (__wasi_addr_port_t *)(uintptr_t)0xFFFFFFFFu;
    err = __wasi_sock_bind(fd, bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

static void test_invalid_address_family(void)
{
    // bind01: EAFNOSUPPORT -> invalid address family here is INVAL.
    printf("Test 4: invalid address family\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t addr;
    memset(&addr, 0, sizeof(addr));
    addr.tag = __WASI_ADDRESS_FAMILY_UNIX;

    err = __wasi_sock_bind(fd, &addr);
    assert(err == __WASI_ERRNO_INVAL);

    close(fd);
}

static void test_family_mismatch(void)
{
    // IP version mismatch should return INVAL.
    printf("Test 5: address family mismatch\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t addr6;
    uint8_t loopback6[16] = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1};
    set_ipv6_addr_port_le(&addr6, 0, loopback6);

    err = __wasi_sock_bind(fd, &addr6);
    assert(err == __WASI_ERRNO_INVAL);

    close(fd);
}

static void test_bind_any_port_zero(void)
{
    // From LTP bind01: bind to INADDR_ANY with port 0 succeeds.
    printf("Test 6: bind INADDR_ANY:0\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t addr;
    set_ipv4_addr_port_le(&addr, 0, 0, 0, 0, 0);
    err = __wasi_sock_bind(fd, &addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    close(fd);
}

static void test_bind_non_local_addr(void)
{
    // From LTP bind01: non-local address should return ADDRNOTAVAIL.
    printf("Test 7: bind non-local address\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t addr;
    // 203.0.113.1 is TEST-NET-3 (non-routable)
    set_ipv4_addr_port_le(&addr, 0, 203, 0, 113, 1);
    err = __wasi_sock_bind(fd, &addr);
    if (err != __WASI_ERRNO_ADDRNOTAVAIL) {
        fprintf(stderr, "Expected ADDRNOTAVAIL, got %u\n", err);
        fflush(stderr);
        assert(0);
    }

    close(fd);
}

int main(void)
{
    printf("WASIX sock_bind integration tests\n");
    test_invalid_fd();
    test_not_socket();
    test_invalid_pointer();
    test_invalid_address_family();
    test_family_mismatch();
    test_bind_any_port_zero();
    test_bind_non_local_addr();
    printf("All tests passed!\n");
    return 0;
}
