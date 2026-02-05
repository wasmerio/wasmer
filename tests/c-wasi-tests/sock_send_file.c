#include <assert.h>
#include <fcntl.h>
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

static void open_connected_tcp(__wasi_fd_t *client_fd, __wasi_fd_t *server_fd,
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

static void close_pair(__wasi_fd_t client_fd, __wasi_fd_t server_fd,
                       __wasi_fd_t accepted_fd)
{
    close(accepted_fd);
    close(client_fd);
    close(server_fd);
}

static void recv_exact(__wasi_fd_t fd, void *out, size_t len)
{
    uint8_t *bytes = (uint8_t *)out;
    size_t offset = 0;
    while (offset < len) {
        __wasi_iovec_t iov = {
            .buf = bytes + offset,
            .buf_len = len - offset,
        };
        __wasi_size_t nread = 0;
        __wasi_roflags_t roflags = 0;
        __wasi_errno_t err = __wasi_sock_recv(fd, &iov, 1, 0, &nread, &roflags);
        assert(err == __WASI_ERRNO_SUCCESS);
        assert(nread > 0);
        offset += nread;
    }
}

static int create_input_file(const char *name)
{
    const char *data = "abcdefghijklmnopqrstuvwxyz";
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(write(fd, data, strlen(data)) == (ssize_t)strlen(data));
    return fd;
}

static void test_invalid_fd(void)
{
    printf("Test 1: invalid out fd\n");
    int in_fd = create_input_file("sendfile_in");
    __wasi_filesize_t sent = 0;
    __wasi_errno_t err = __wasi_sock_send_file(9999, in_fd, 0, 1, &sent);
    expect_errno("invalid out fd", err, __WASI_ERRNO_BADF);
    close(in_fd);
}

static void test_not_socket(void)
{
    printf("Test 2: out fd not a socket\n");
    int out_fd = open("sendfile_out", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(out_fd >= 0);
    int in_fd = create_input_file("sendfile_in");
    __wasi_filesize_t sent = 0;
    __wasi_errno_t err = __wasi_sock_send_file((__wasi_fd_t)out_fd, in_fd, 0, 1, &sent);
    expect_errno("not socket", err, __WASI_ERRNO_NOTSOCK);
    close(in_fd);
    close(out_fd);
    unlink("sendfile_out");
}

static void test_offset_zero_full(void)
{
    printf("Test 3: offset 0 full copy\n");
    int in_fd = create_input_file("sendfile_in");

    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    __wasi_filesize_t sent = 0;
    __wasi_errno_t err = __wasi_sock_send_file(client_fd, in_fd, 0, 26, &sent);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(sent == 26);

    char buf[27] = {0};
    recv_exact(accepted_fd, buf, 26);
    assert(strncmp(buf, "abcdefghijklmnopqrstuvwxyz", 26) == 0);

    __wasi_filesize_t pos = 0;
    err = __wasi_fd_tell(in_fd, &pos);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pos == 26);

    close_pair(client_fd, server_fd, accepted_fd);
    close(in_fd);
}

static void test_offset_mid(void)
{
    printf("Test 4: offset mid copy\n");
    int in_fd = create_input_file("sendfile_in");

    __wasi_fd_t client_fd = 0;
    __wasi_fd_t server_fd = 0;
    __wasi_fd_t accepted_fd = 0;
    open_connected_tcp(&client_fd, &server_fd, &accepted_fd);

    __wasi_filesize_t sent = 0;
    __wasi_errno_t err = __wasi_sock_send_file(client_fd, in_fd, 2, 4, &sent);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(sent == 4);

    char buf[5] = {0};
    recv_exact(accepted_fd, buf, 4);
    assert(strncmp(buf, "cdef", 4) == 0);

    __wasi_filesize_t pos = 0;
    err = __wasi_fd_tell(in_fd, &pos);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pos == 6);

    close_pair(client_fd, server_fd, accepted_fd);
    close(in_fd);
}

int main(void)
{
    printf("WASIX sock_send_file integration tests\n");
    test_invalid_fd();
    test_not_socket();
    test_offset_zero_full();
    test_offset_mid();

    if (failures != 0) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("All tests passed!\n");
    return 0;
}
