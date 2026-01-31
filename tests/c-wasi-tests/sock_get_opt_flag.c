#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
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

static void test_invalid_fd(void)
{
    printf("Test 1: invalid fd\n");
    __wasi_bool_t flag = __WASI_BOOL_FALSE;
    __wasi_errno_t err = __wasi_sock_get_opt_flag(
        9999,
        __WASI_SOCK_OPTION_REUSE_ADDR,
        &flag);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_not_socket(void)
{
    printf("Test 2: not a socket\n");
    int fd = open("sock_get_opt_flag_file", O_CREAT | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_bool_t flag = __WASI_BOOL_FALSE;
    __wasi_errno_t err = __wasi_sock_get_opt_flag(
        (__wasi_fd_t)fd,
        __WASI_SOCK_OPTION_REUSE_ADDR,
        &flag);
    assert(err == __WASI_ERRNO_NOTSOCK);

    close(fd);
    assert(unlink("sock_get_opt_flag_file") == 0);
}

static void test_defaults_and_set_get(void)
{
    printf("Test 3: default flags and set/get\n");
    __wasi_fd_t fd = open_tcp_socket();

    __wasi_bool_t flag = __WASI_BOOL_TRUE;
    __wasi_errno_t err = __wasi_sock_get_opt_flag(
        fd,
        __WASI_SOCK_OPTION_REUSE_ADDR,
        &flag);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(flag == __WASI_BOOL_FALSE);

    err = __wasi_sock_set_opt_flag(
        fd,
        __WASI_SOCK_OPTION_REUSE_ADDR,
        __WASI_BOOL_TRUE);
    assert(err == __WASI_ERRNO_SUCCESS);
    flag = __WASI_BOOL_FALSE;
    err = __wasi_sock_get_opt_flag(
        fd,
        __WASI_SOCK_OPTION_REUSE_ADDR,
        &flag);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(flag == __WASI_BOOL_TRUE);

    err = __wasi_sock_set_opt_flag(
        fd,
        __WASI_SOCK_OPTION_REUSE_ADDR,
        __WASI_BOOL_FALSE);
    assert(err == __WASI_ERRNO_SUCCESS);
    flag = __WASI_BOOL_TRUE;
    err = __wasi_sock_get_opt_flag(
        fd,
        __WASI_SOCK_OPTION_REUSE_ADDR,
        &flag);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(flag == __WASI_BOOL_FALSE);

    flag = __WASI_BOOL_TRUE;
    err = __wasi_sock_get_opt_flag(
        fd,
        __WASI_SOCK_OPTION_KEEP_ALIVE,
        &flag);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(flag == __WASI_BOOL_FALSE);

    err = __wasi_sock_set_opt_flag(
        fd,
        __WASI_SOCK_OPTION_KEEP_ALIVE,
        __WASI_BOOL_TRUE);
    assert(err == __WASI_ERRNO_SUCCESS);
    flag = __WASI_BOOL_FALSE;
    err = __wasi_sock_get_opt_flag(
        fd,
        __WASI_SOCK_OPTION_KEEP_ALIVE,
        &flag);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(flag == __WASI_BOOL_TRUE);

    close(fd);
}

static void test_invalid_option(void)
{
    printf("Test 4: invalid option\n");
    __wasi_fd_t fd = open_tcp_socket();
    __wasi_bool_t flag = __WASI_BOOL_FALSE;

    __wasi_sock_option_t bad_opt = (__wasi_sock_option_t)0xFFu;
    __wasi_errno_t err = __wasi_sock_get_opt_flag(fd, bad_opt, &flag);
    assert(err == __WASI_ERRNO_INVAL);

    err = __wasi_sock_get_opt_flag(
        fd,
        __WASI_SOCK_OPTION_BROADCAST,
        &flag);
    assert(err == __WASI_ERRNO_INVAL);

    close(fd);
}

static void test_invalid_pointer(void)
{
    printf("Test 5: invalid pointer\n");
    __wasi_fd_t fd = open_tcp_socket();

    __wasi_bool_t *bad_ptr = (__wasi_bool_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_sock_get_opt_flag(
        fd,
        __WASI_SOCK_OPTION_REUSE_ADDR,
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
