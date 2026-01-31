#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

static void test_fd_advise_basic(void)
{
    // From wasmtime p1_fd_advise.rs.
    printf("Test 1: fd_advise basic flow\n");
    int fd = open("fd_advise_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    int rc = ftruncate(fd, 100);
    assert(rc == 0);

    __wasi_errno_t err = __wasi_fd_advise((__wasi_fd_t)fd, 10, 50, __WASI_ADVICE_NORMAL);
    assert(err == __WASI_ERRNO_SUCCESS);

    off_t size = lseek(fd, 0, SEEK_END);
    assert(size == 100);

    close(fd);
    rc = unlink("fd_advise_file");
    assert(rc == 0);
}

static void test_fd_advise_all_advice(void)
{
    // From LTP posix_fadvise01.c: all valid advice values should succeed.
    printf("Test 2: fd_advise all advice values\n");
    static const __wasi_advice_t advice_values[] = {
        __WASI_ADVICE_NORMAL,
        __WASI_ADVICE_SEQUENTIAL,
        __WASI_ADVICE_RANDOM,
        __WASI_ADVICE_NOREUSE,
        __WASI_ADVICE_WILLNEED,
        __WASI_ADVICE_DONTNEED,
    };

    int fd = open("fd_advise_all", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    for (size_t i = 0; i < sizeof(advice_values) / sizeof(advice_values[0]); i++) {
        __wasi_errno_t err = __wasi_fd_advise((__wasi_fd_t)fd, 0, 0, advice_values[i]);
        assert(err == __WASI_ERRNO_SUCCESS);
    }

    close(fd);
    assert(unlink("fd_advise_all") == 0);
}

static void test_fd_advise_dir_ok(void)
{
    // Linux allows posix_fadvise on directories to succeed.
    printf("Test 3: fd_advise on directory (Linux-compatible)\n");
    int fd = open(".", O_RDONLY);
    assert(fd >= 0);
    __wasi_errno_t err = __wasi_fd_advise((__wasi_fd_t)fd, 0, 0, __WASI_ADVICE_DONTNEED);
    assert(err == __WASI_ERRNO_SUCCESS);
    close(fd);
}

static void test_fd_advise_invalid_fd(void)
{
    // From LTP posix_fadvise02.c: EBADF for invalid fd on all advice values.
    printf("Test 4: fd_advise invalid fd (EBADF)\n");
    static const __wasi_advice_t advice_values[] = {
        __WASI_ADVICE_NORMAL,
        __WASI_ADVICE_SEQUENTIAL,
        __WASI_ADVICE_RANDOM,
        __WASI_ADVICE_NOREUSE,
        __WASI_ADVICE_WILLNEED,
        __WASI_ADVICE_DONTNEED,
    };

    for (size_t i = 0; i < sizeof(advice_values) / sizeof(advice_values[0]); i++) {
        __wasi_errno_t err = __wasi_fd_advise((__wasi_fd_t)9999, 0, 0, advice_values[i]);
        assert(err == __WASI_ERRNO_BADF);
    }
}

static void test_fd_advise_invalid_advice(void)
{
    // From LTP posix_fadvise03.c: invalid advice should return EINVAL.
    printf("Test 5: fd_advise invalid advice (EINVAL)\n");
    int fd = open("fd_advise_invalid_advice", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_advise((__wasi_fd_t)fd, 0, 0, (__wasi_advice_t)999);
    assert(err == __WASI_ERRNO_INVAL);

    close(fd);
    assert(unlink("fd_advise_invalid_advice") == 0);
}

static void test_fd_advise_overflow(void)
{
    printf("Test 6: fd_advise offset+len overflow\n");
    int fd = open("fd_advise_overflow", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_filesize_t offset = UINT64_C(0xFFFFFFFFFFFFFFFF);
    __wasi_filesize_t len = 1;
    __wasi_errno_t err = __wasi_fd_advise((__wasi_fd_t)fd, offset, len, __WASI_ADVICE_NORMAL);
    assert(err == __WASI_ERRNO_INVAL);

    close(fd);
    unlink("fd_advise_overflow");
}

int main(void)
{
    test_fd_advise_basic();
    test_fd_advise_all_advice();
    test_fd_advise_dir_ok();
    test_fd_advise_invalid_fd();
    test_fd_advise_invalid_advice();
    test_fd_advise_overflow();
    printf("All tests passed!\n");
    return 0;
}
