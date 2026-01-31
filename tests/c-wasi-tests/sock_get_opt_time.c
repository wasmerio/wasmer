#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api_wasix.h>

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

static __wasi_option_timestamp_t make_none(void)
{
    __wasi_option_timestamp_t t;
    memset(&t, 0, sizeof(t));
    t.tag = __WASI_OPTION_NONE;
    t.u.none = 0;
    return t;
}

static __wasi_option_timestamp_t make_some(__wasi_timestamp_t ns)
{
    __wasi_option_timestamp_t t;
    memset(&t, 0, sizeof(t));
    t.tag = __WASI_OPTION_SOME;
    t.u.some = ns;
    return t;
}

static void assert_none(const __wasi_option_timestamp_t *t)
{
    assert(t->tag == __WASI_OPTION_NONE);
}

static void assert_some(const __wasi_option_timestamp_t *t, __wasi_timestamp_t expected)
{
    assert(t->tag == __WASI_OPTION_SOME);
    assert(t->u.some == expected);
}

static void test_invalid_fd(void)
{
    printf("Test 1: invalid fd\n");
    __wasi_option_timestamp_t out = make_none();
    __wasi_errno_t err = __wasi_sock_get_opt_time(
        9999,
        __WASI_SOCK_OPTION_RECV_TIMEOUT,
        &out);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    printf("Test 2: not a socket\n");
    int fd = open("sock_get_opt_time_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_option_timestamp_t out = make_none();
    __wasi_errno_t err = __wasi_sock_get_opt_time(
        (__wasi_fd_t)fd,
        __WASI_SOCK_OPTION_RECV_TIMEOUT,
        &out);
    assert(err == __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_get_opt_time_file") == 0);
}

static void test_defaults_and_set_get(void)
{
    printf("Test 3: defaults and set/get timeouts\n");
    __wasi_fd_t fd = open_tcp_socket();

    __wasi_option_timestamp_t out = make_some(123);
    __wasi_errno_t err = __wasi_sock_get_opt_time(
        fd,
        __WASI_SOCK_OPTION_RECV_TIMEOUT,
        &out);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_none(&out);

    out = make_some(456);
    err = __wasi_sock_get_opt_time(
        fd,
        __WASI_SOCK_OPTION_CONNECT_TIMEOUT,
        &out);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_none(&out);

    __wasi_option_timestamp_t set_val = make_some(250000000);
    err = __wasi_sock_set_opt_time(
        fd,
        __WASI_SOCK_OPTION_RECV_TIMEOUT,
        &set_val);
    assert(err == __WASI_ERRNO_SUCCESS);

    set_val = make_some(500000000);
    err = __wasi_sock_set_opt_time(
        fd,
        __WASI_SOCK_OPTION_SEND_TIMEOUT,
        &set_val);
    assert(err == __WASI_ERRNO_SUCCESS);

    set_val = make_some(750000000);
    err = __wasi_sock_set_opt_time(
        fd,
        __WASI_SOCK_OPTION_CONNECT_TIMEOUT,
        &set_val);
    assert(err == __WASI_ERRNO_SUCCESS);

    set_val = make_some(1250000000);
    err = __wasi_sock_set_opt_time(
        fd,
        __WASI_SOCK_OPTION_ACCEPT_TIMEOUT,
        &set_val);
    assert(err == __WASI_ERRNO_SUCCESS);

    out = make_none();
    err = __wasi_sock_get_opt_time(
        fd,
        __WASI_SOCK_OPTION_RECV_TIMEOUT,
        &out);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_some(&out, 250000000);

    out = make_none();
    err = __wasi_sock_get_opt_time(
        fd,
        __WASI_SOCK_OPTION_SEND_TIMEOUT,
        &out);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_some(&out, 500000000);

    out = make_none();
    err = __wasi_sock_get_opt_time(
        fd,
        __WASI_SOCK_OPTION_CONNECT_TIMEOUT,
        &out);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_some(&out, 750000000);

    out = make_none();
    err = __wasi_sock_get_opt_time(
        fd,
        __WASI_SOCK_OPTION_ACCEPT_TIMEOUT,
        &out);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_some(&out, 1250000000);

    set_val = make_none();
    err = __wasi_sock_set_opt_time(
        fd,
        __WASI_SOCK_OPTION_RECV_TIMEOUT,
        &set_val);
    assert(err == __WASI_ERRNO_SUCCESS);

    out = make_some(777);
    err = __wasi_sock_get_opt_time(
        fd,
        __WASI_SOCK_OPTION_RECV_TIMEOUT,
        &out);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_none(&out);

    close(fd);
}

static void test_invalid_option(void)
{
    printf("Test 4: invalid option\n");
    __wasi_fd_t fd = open_tcp_socket();
    __wasi_option_timestamp_t out = make_none();

    __wasi_errno_t err = __wasi_sock_get_opt_time(
        fd,
        __WASI_SOCK_OPTION_REUSE_ADDR,
        &out);
    assert(err == __WASI_ERRNO_INVAL);

    err = __wasi_sock_get_opt_time(
        fd,
        __WASI_SOCK_OPTION_LINGER,
        &out);
    assert(err == __WASI_ERRNO_INVAL);

    close(fd);
}

static void test_invalid_pointer(void)
{
    printf("Test 5: invalid pointer\n");
    __wasi_fd_t fd = open_tcp_socket();

    __wasi_option_timestamp_t *bad_ptr = (__wasi_option_timestamp_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_sock_get_opt_time(
        fd,
        __WASI_SOCK_OPTION_RECV_TIMEOUT,
        bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);

    close(fd);
}

int main(void)
{
    test_invalid_fd();
    test_not_socket();
    test_defaults_and_set_get();
    test_invalid_option();
    test_invalid_pointer();
    printf("All tests passed!\n");
    return 0;
}
