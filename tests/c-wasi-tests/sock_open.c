#include <assert.h>
#include <stdint.h>
#include <stdio.h>
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

static void test_invalid_domain(void)
{
    // From LTP socket01: EAFNOSUPPORT on invalid domain.
    printf("Test 1: invalid domain\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open((__wasi_address_family_t)0xFF,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    expect_errno("invalid domain", err, __WASI_ERRNO_AFNOSUPPORT);
}

static void test_invalid_type(void)
{
    // From LTP socket01: EINVAL on invalid type.
    printf("Test 2: invalid type\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          (__wasi_sock_type_t)0xFF,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    expect_errno("invalid type", err, __WASI_ERRNO_INVAL);
}

static void test_unix_domain(void)
{
    // From LTP socket01: UNIX domain is unsupported in WASIX.
    printf("Test 3: UNIX domain not supported\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_UNIX,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_IP,
                                          &fd);
    expect_errno("unix domain", err, __WASI_ERRNO_AFNOSUPPORT);
}

static void test_raw_non_root(void)
{
    // From LTP socket01: raw socket open should be PROTONOSUPPORT.
    printf("Test 4: raw socket not supported\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_RAW,
                                          __WASI_SOCK_PROTO_IP,
                                          &fd);
    expect_errno("raw socket", err, __WASI_ERRNO_PROTONOSUPPORT);
}

static void test_udp_socket(void)
{
    // From LTP socket01: UDP socket opens successfully.
    printf("Test 5: UDP socket\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &fd);
    expect_errno("udp socket", err, __WASI_ERRNO_SUCCESS);
    close(fd);
}

static void test_udp_stream(void)
{
    // From LTP socket01: UDP stream should be PROTONOSUPPORT.
    printf("Test 6: UDP stream not supported\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &fd);
    expect_errno("udp stream", err, __WASI_ERRNO_PROTONOSUPPORT);
}

static void test_tcp_dgram(void)
{
    // From LTP socket01: TCP datagram should be PROTONOSUPPORT.
    printf("Test 7: TCP datagram not supported\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    expect_errno("tcp dgram", err, __WASI_ERRNO_PROTONOSUPPORT);
}

static void test_tcp_socket(void)
{
    // From LTP socket01: TCP socket opens successfully.
    printf("Test 8: TCP socket\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd);
    expect_errno("tcp socket", err, __WASI_ERRNO_SUCCESS);
    close(fd);
}

static void test_icmp_stream(void)
{
    // From LTP socket01: ICMP stream should be PROTONOSUPPORT.
    printf("Test 9: ICMP stream not supported\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_ICMP,
                                          &fd);
    expect_errno("icmp stream", err, __WASI_ERRNO_PROTONOSUPPORT);
}

static void test_invalid_ptr(void)
{
    // Invalid fd pointer should be MEMVIOLATION.
    printf("Test 10: invalid fd pointer\n");
    __wasi_fd_t *bad_ptr = (__wasi_fd_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          bad_ptr);
    expect_errno("invalid fd pointer", err, __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    printf("WASIX sock_open integration tests\n");
    test_invalid_domain();
    test_invalid_type();
    test_unix_domain();
    test_raw_non_root();
    test_udp_socket();
    test_udp_stream();
    test_tcp_dgram();
    test_tcp_socket();
    test_icmp_stream();
    test_invalid_ptr();
    if (failures != 0) {
        fprintf(stderr, "%d sock_open check(s) failed\n", failures);
        assert(0);
    }
    printf("All tests passed!\n");
    return 0;
}
