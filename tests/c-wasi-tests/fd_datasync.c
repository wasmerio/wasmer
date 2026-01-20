#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

static void write_all(int fd, const void *buf, size_t len)
{
    const char *cursor = (const char *)buf;
    size_t remaining = len;

    while (remaining > 0) {
        ssize_t written = write(fd, cursor, remaining);
        assert(written > 0);
        cursor += (size_t)written;
        remaining -= (size_t)written;
    }
}

static void test_fd_datasync_basic(void)
{
    // From LTP fdatasync01.c and stress-ng test-fdatasync.c.
    printf("Test 1: fd_datasync basic flow\n");
    int fd = open("fd_datasync_basic", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    const char payload[] = "fd_datasync basic";
    write_all(fd, payload, sizeof(payload));

    __wasi_errno_t err = __wasi_fd_datasync((__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    close(fd);
    assert(unlink("fd_datasync_basic") == 0);
}

static void test_fd_datasync_large_write(void)
{
    // From LTP fdatasync03.c (large dirty pages).
    printf("Test 2: fd_datasync large write\n");
    int fd = open("fd_datasync_large", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    size_t size = 1024 * 1024;
    char *buffer = (char *)malloc(size);
    assert(buffer != NULL);
    memset(buffer, 'A', size);
    write_all(fd, buffer, size);
    free(buffer);

    __wasi_errno_t err = __wasi_fd_datasync((__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    off_t end = lseek(fd, 0, SEEK_END);
    assert(end == (off_t)size);

    close(fd);
    assert(unlink("fd_datasync_large") == 0);
}

static void test_fd_datasync_invalid_fd(void)
{
    // From LTP fdatasync02.c.
    printf("Test 3: fd_datasync invalid fd\n");
    __wasi_errno_t err = __wasi_fd_datasync((__wasi_fd_t)9999);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_fd_datasync_dir(void)
{
    // From wasmtime p1_dir_fd_op_failures.rs (Linux allows success).
    printf("Test 4: fd_datasync on directory (Linux-compatible)\n");
    int fd = open(".", O_RDONLY);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_datasync((__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    close(fd);
}

static void test_fd_datasync_special_file(void)
{
    // From LTP fdatasync02.c (special file).
    printf("Test 5: fd_datasync on special file\n");
    int fd = open("/dev/null", O_RDONLY);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_datasync((__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_INVAL);

    close(fd);
}

int main(void)
{
    test_fd_datasync_basic();
    test_fd_datasync_large_write();
    test_fd_datasync_invalid_fd();
    test_fd_datasync_dir();
    test_fd_datasync_special_file();
    printf("All tests passed!\n");
    return 0;
}
