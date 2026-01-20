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

static void test_fd_advise_dir_badf(void)
{
    // Linux allows posix_fadvise on directories to succeed.
    printf("Test 2: fd_advise on directory (Linux-compatible)\n");
    int fd = open(".", O_RDONLY);
    assert(fd >= 0);
    __wasi_errno_t err = __wasi_fd_advise((__wasi_fd_t)fd, 0, 0, __WASI_ADVICE_DONTNEED);
    assert(err == __WASI_ERRNO_SUCCESS);
    close(fd);
}

static void test_fd_advise_invalid_fd(void)
{
    printf("Test 3: fd_advise invalid fd\n");
    __wasi_errno_t err = __wasi_fd_advise((__wasi_fd_t)9999, 0, 0, __WASI_ADVICE_NORMAL);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_fd_advise_overflow(void)
{
    printf("Test 4: fd_advise offset+len overflow\n");
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
    test_fd_advise_dir_badf();
    test_fd_advise_invalid_fd();
    test_fd_advise_overflow();
    printf("All tests passed!\n");
    return 0;
}
