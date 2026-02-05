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

static void test_invalid_domain(void)
{
    printf("Test 1: invalid domain\n");
    __wasi_fd_t fd1 = 0, fd2 = 0;
    __wasi_errno_t err = __wasi_sock_pair((__wasi_address_family_t)0xFF,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd1, &fd2);
    expect_errno("invalid domain", err, __WASI_ERRNO_AFNOSUPPORT);
}

static void test_invalid_type(void)
{
    printf("Test 2: invalid type\n");
    __wasi_fd_t fd1 = 0, fd2 = 0;
    __wasi_errno_t err = __wasi_sock_pair(__WASI_ADDRESS_FAMILY_INET4,
                                          (__wasi_sock_type_t)0xFF,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd1, &fd2);
    expect_errno("invalid type", err, __WASI_ERRNO_INVAL);
}

static void test_invalid_pointer(void)
{
    printf("Test 3: invalid pointer\n");
    __wasi_fd_t fd1 = 0;
    __wasi_fd_t *bad_ptr = (__wasi_fd_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_sock_pair(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          bad_ptr, &fd1);
    expect_errno("invalid pointer", err, __WASI_ERRNO_MEMVIOLATION);
}

static void test_proto_mismatch(void)
{
    printf("Test 4: protocol/type mismatch\n");
    __wasi_fd_t fd1 = 0, fd2 = 0;
    __wasi_errno_t err = __wasi_sock_pair(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &fd1, &fd2);
    expect_errno("proto mismatch", err, __WASI_ERRNO_NOTSUP);
}

static void test_stream_pair_basic(void)
{
    printf("Test 5: stream pair basic\n");
    __wasi_fd_t fd1 = 0, fd2 = 0;
    __wasi_errno_t err = __wasi_sock_pair(__WASI_ADDRESS_FAMILY_INET4,
                                          __WASI_SOCK_TYPE_SOCKET_STREAM,
                                          __WASI_SOCK_PROTO_TCP,
                                          &fd1, &fd2);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(fd1 >= 0);
    assert(fd2 >= 0);
    assert(fd1 != fd2);

    const char msg1[] = "hello";
    char buf1[8] = {0};
    ssize_t wrote = write(fd1, msg1, sizeof(msg1));
    assert(wrote == (ssize_t)sizeof(msg1));
    ssize_t readn = read(fd2, buf1, sizeof(buf1));
    assert(readn == (ssize_t)sizeof(msg1));
    assert(memcmp(buf1, msg1, sizeof(msg1)) == 0);

    const char msg2[] = "world";
    char buf2[8] = {0};
    wrote = write(fd2, msg2, sizeof(msg2));
    assert(wrote == (ssize_t)sizeof(msg2));
    readn = read(fd1, buf2, sizeof(buf2));
    assert(readn == (ssize_t)sizeof(msg2));
    assert(memcmp(buf2, msg2, sizeof(msg2)) == 0);

    close(fd1);
    close(fd2);
}

static void test_unix_dgram_pair_basic(void)
{
    printf("Test 6: UNIX dgram pair basic\n");
    __wasi_fd_t fd1 = 0, fd2 = 0;
    __wasi_errno_t err = __wasi_sock_pair(__WASI_ADDRESS_FAMILY_UNIX,
                                          __WASI_SOCK_TYPE_SOCKET_DGRAM,
                                          __WASI_SOCK_PROTO_UDP,
                                          &fd1, &fd2);
    assert(err == __WASI_ERRNO_SUCCESS);

    const char msg[] = "ping";
    char buf[8] = {0};
    ssize_t wrote = write(fd1, msg, sizeof(msg));
    assert(wrote == (ssize_t)sizeof(msg));
    ssize_t readn = read(fd2, buf, sizeof(buf));
    assert(readn == (ssize_t)sizeof(msg));
    assert(memcmp(buf, msg, sizeof(msg)) == 0);

    close(fd1);
    close(fd2);
}

int main(void)
{
    printf("WASIX sock_pair integration tests\n");

    test_invalid_domain();
    test_invalid_type();
    test_invalid_pointer();
    test_proto_mismatch();
    test_stream_pair_basic();
    test_unix_dgram_pair_basic();

    if (failures) {
        fprintf(stderr, "%d test(s) failed\n", failures);
        return 1;
    }

    printf("All tests passed!\n");
    return 0;
}
