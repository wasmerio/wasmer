#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

#ifndef __WASI_SOCK_RECV_INPUT_DONT_WAIT
#define __WASI_SOCK_RECV_INPUT_DONT_WAIT ((__wasi_riflags_t)(1 << 3))
#endif

static int failures = 0;

static void expect_errno(const char *name, __wasi_errno_t got,
                         __wasi_errno_t expect)
{
    if (got != expect) {
        fprintf(stderr, "%s: expected %u, got %u\n", name, expect, got);
        failures++;
    }
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

static uint16_t port_from_addr_be(const __wasi_addr_port_t *addr)
{
    const unsigned char *octs = (const unsigned char *)&addr->u;
    return (uint16_t)(((uint16_t)octs[0] << 8) | (uint16_t)octs[1]);
}

static __wasi_errno_t recv_from_retry(__wasi_fd_t fd, __wasi_iovec_t *iov,
                                      __wasi_size_t iov_len,
                                      __wasi_riflags_t flags,
                                      __wasi_size_t *nread,
                                      __wasi_roflags_t *roflags,
                                      __wasi_addr_port_t *peer)
{
    for (int i = 0; i < 1000; i++) {
        __wasi_errno_t err =
            __wasi_sock_recv_from(fd, iov, iov_len,
                                  flags | __WASI_SOCK_RECV_INPUT_DONT_WAIT,
                                  nread, roflags, peer);
        if (err == __WASI_ERRNO_AGAIN) {
            continue;
        }
        return err;
    }
    return __WASI_ERRNO_AGAIN;
}

static __wasi_errno_t send_to_retry(__wasi_fd_t fd, const __wasi_ciovec_t *iov,
                                    __wasi_size_t iov_len,
                                    __wasi_siflags_t flags,
                                    const __wasi_addr_port_t *addr,
                                    __wasi_size_t *nsent)
{
    for (int i = 0; i < 100; i++) {
        __wasi_errno_t err =
            __wasi_sock_send_to(fd, iov, iov_len, flags, addr, nsent);
        if (err == __WASI_ERRNO_INTR) {
            continue;
        }
        return err;
    }
    return __WASI_ERRNO_INTR;
}

static void open_udp_server(__wasi_fd_t *server_fd, __wasi_addr_port_t *addr)
{
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          server_fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_addr_port_t bind_addr;
    set_ipv4_addr_port_le(&bind_addr, 0, 127, 0, 0, 1);
    err = __wasi_sock_bind(*server_fd, &bind_addr);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_sock_addr_local(*server_fd, addr);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(addr->tag == __WASI_ADDRESS_FAMILY_INET4);
    assert(port_from_addr_be(addr) != 0);
}

static void open_udp_client(__wasi_fd_t *client_fd)
{
    __wasi_errno_t err = __wasi_sock_open(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          client_fd);
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_invalid_fd(void)
{
    printf("Test 1: invalid fd\n");
    const uint8_t msg[] = "x";
    __wasi_ciovec_t iov = {.buf = msg, .buf_len = sizeof(msg)};
    __wasi_size_t nsent = 0;
    __wasi_addr_port_t addr;
    set_ipv4_addr_port_le(&addr, 1234, 127, 0, 0, 1);

    __wasi_errno_t err =
        __wasi_sock_send_to(9999, &iov, 1, 0, &addr, &nsent);
    expect_errno("invalid fd", err, __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    printf("Test 2: not a socket\n");
    int fd = open("sock_send_to_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    const uint8_t msg[] = "x";
    __wasi_ciovec_t iov = {.buf = msg, .buf_len = sizeof(msg)};
    __wasi_size_t nsent = 0;
    __wasi_addr_port_t addr;
    set_ipv4_addr_port_le(&addr, 1234, 127, 0, 0, 1);

    __wasi_errno_t err =
        __wasi_sock_send_to((__wasi_fd_t)fd, &iov, 1, 0, &addr, &nsent);
    expect_errno("not socket", err, __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_send_to_file") == 0);
}

static void test_invalid_iovec(void)
{
    printf("Test 3: invalid iovec pointer\n");
    __wasi_fd_t fd = 0;
    open_udp_client(&fd);

    __wasi_ciovec_t *bad_iov = (__wasi_ciovec_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_size_t nsent = 0;
    __wasi_addr_port_t addr;
    set_ipv4_addr_port_le(&addr, 1234, 127, 0, 0, 1);

    __wasi_errno_t err =
        __wasi_sock_send_to(fd, bad_iov, 1, 0, &addr, &nsent);
    expect_errno("invalid iovec", err, __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

static void test_invalid_buffer(void)
{
    printf("Test 4: invalid buffer\n");
    __wasi_fd_t fd = 0;
    open_udp_client(&fd);

    __wasi_ciovec_t iov = {
        .buf = (const uint8_t *)0xFFFFF000u,
        .buf_len = 4,
    };
    __wasi_size_t nsent = 0;
    __wasi_addr_port_t addr;
    set_ipv4_addr_port_le(&addr, 1234, 127, 0, 0, 1);

    __wasi_errno_t err =
        __wasi_sock_send_to(fd, &iov, 1, 0, &addr, &nsent);
    expect_errno("invalid buffer", err, __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

static void test_invalid_addr(void)
{
    printf("Test 5: invalid addr pointer\n");
    __wasi_fd_t fd = 0;
    open_udp_client(&fd);

    const uint8_t msg[] = "x";
    __wasi_ciovec_t iov = {.buf = msg, .buf_len = sizeof(msg)};
    __wasi_size_t nsent = 0;
    __wasi_addr_port_t *bad_addr = (__wasi_addr_port_t *)(uintptr_t)0xFFFFFFFFu;

    __wasi_errno_t err =
        __wasi_sock_send_to(fd, &iov, 1, 0, bad_addr, &nsent);
    expect_errno("invalid addr", err, __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

static void test_invalid_addr_family(void)
{
    printf("Test 6: invalid addr family\n");
    __wasi_fd_t fd = 0;
    open_udp_client(&fd);

    const uint8_t msg[] = "x";
    __wasi_ciovec_t iov = {.buf = msg, .buf_len = sizeof(msg)};
    __wasi_size_t nsent = 0;
    __wasi_addr_port_t addr;
    memset(&addr, 0, sizeof(addr));
    addr.tag = __WASI_ADDRESS_FAMILY_UNIX;

    __wasi_errno_t err =
        __wasi_sock_send_to(fd, &iov, 1, 0, &addr, &nsent);
    expect_errno("invalid addr family", err, __WASI_ERRNO_INVAL);

    close(fd);
}

static void test_basic_sendto(void)
{
    printf("Test 7: UDP sendto + recvfrom\n");
    __wasi_fd_t server_fd = 0, client_fd = 0;
    __wasi_addr_port_t server_addr;
    open_udp_server(&server_fd, &server_addr);
    open_udp_client(&client_fd);

    __wasi_addr_port_t dest_addr;
    uint16_t port = port_from_addr_be(&server_addr);
    set_ipv4_addr_port_le(&dest_addr, port, 127, 0, 0, 1);

    const uint8_t msg[] = "hello";
    __wasi_ciovec_t iov = {.buf = msg, .buf_len = sizeof(msg)};
    __wasi_size_t nsent = 0;

    __wasi_errno_t err =
        send_to_retry(client_fd, &iov, 1, 0, &dest_addr, &nsent);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nsent == sizeof(msg));

    uint8_t buf[8] = {0};
    __wasi_iovec_t riov = {.buf = buf, .buf_len = sizeof(buf)};
    __wasi_size_t nread = 0;
    __wasi_roflags_t roflags = 0;
    __wasi_addr_port_t peer;
    err = recv_from_retry(server_fd, &riov, 1, 0, &nread, &roflags, &peer);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nread == sizeof(msg));
    assert(memcmp(buf, msg, sizeof(msg)) == 0);

    close(server_fd);
    close(client_fd);
}

static void test_udp_message_too_big(void)
{
    printf("Test 8: UDP message too big\n");
    __wasi_fd_t server_fd = 0, client_fd = 0;
    __wasi_addr_port_t server_addr;
    open_udp_server(&server_fd, &server_addr);
    open_udp_client(&client_fd);

    __wasi_addr_port_t dest_addr;
    uint16_t port = port_from_addr_be(&server_addr);
    set_ipv4_addr_port_le(&dest_addr, port, 127, 0, 0, 1);

    static uint8_t bigbuf[128 * 1024];
    memset(bigbuf, 0x42, sizeof(bigbuf));

    __wasi_ciovec_t iov = {.buf = bigbuf, .buf_len = sizeof(bigbuf)};
    __wasi_size_t nsent = 0;
    __wasi_errno_t err =
        send_to_retry(client_fd, &iov, 1, 0, &dest_addr, &nsent);
    expect_errno("udp msg too big", err, __WASI_ERRNO_MSGSIZE);

    close(server_fd);
    close(client_fd);
}

static void test_invalid_ro_data_len(void)
{
    printf("Test 9: invalid ro_data_len pointer\n");
    __wasi_fd_t server_fd = 0, client_fd = 0;
    __wasi_addr_port_t server_addr;
    open_udp_server(&server_fd, &server_addr);
    open_udp_client(&client_fd);

    const uint8_t msg[] = "x";
    __wasi_ciovec_t iov = {.buf = msg, .buf_len = sizeof(msg)};
    __wasi_size_t *bad_len = (__wasi_size_t *)(uintptr_t)0xFFFFFFFFu;

    __wasi_errno_t err =
        __wasi_sock_send_to(client_fd, &iov, 1, 0, &server_addr, bad_len);
    expect_errno("invalid ro_data_len", err, __WASI_ERRNO_MEMVIOLATION);

    close(server_fd);
    close(client_fd);
}

int main(void)
{
    printf("WASIX sock_send_to integration tests\n");

    test_invalid_fd();
    test_not_socket();
    test_invalid_iovec();
    test_invalid_buffer();
    test_invalid_addr();
    test_invalid_addr_family();
    test_basic_sendto();
    test_udp_message_too_big();
    test_invalid_ro_data_len();

    if (failures) {
        fprintf(stderr, "%d test(s) failed\n", failures);
        return 1;
    }

    printf("All tests passed!\n");
    return 0;
}
