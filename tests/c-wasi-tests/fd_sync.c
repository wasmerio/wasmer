#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
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

static void test_fd_sync_basic(void)
{
    // From LTP fsync01.c.
    printf("Test 1: fd_sync basic file\n");
    int fd = open("fd_sync_basic", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    const char payload[] = "fd_sync basic";
    write_all(fd, payload, sizeof(payload));

    __wasi_errno_t err = __wasi_fd_sync((__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    close(fd);
    assert(unlink("fd_sync_basic") == 0);
}

static void test_fd_sync_closed_fd(void)
{
    // From LTP fsync03.c (EBADF on closed fd).
    printf("Test 2: fd_sync closed fd\n");
    int fd = open("fd_sync_closed", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    close(fd);

    __wasi_errno_t err = __wasi_fd_sync((__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_BADF);

    assert(unlink("fd_sync_closed") == 0);
}

static void test_fd_sync_invalid_fd(void)
{
    // From LTP fsync03.c (EBADF on invalid fd).
    printf("Test 3: fd_sync invalid fd\n");
    __wasi_errno_t err = __wasi_fd_sync((__wasi_fd_t)9999);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_fd_sync_directory(void)
{
    // Linux allows fsync on directories.
    printf("Test 4: fd_sync on directory (Linux-compatible)\n");
    int fd = open(".", O_RDONLY);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_sync((__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    close(fd);
}

int main(void)
{
    test_fd_sync_basic();
    test_fd_sync_closed_fd();
    test_fd_sync_invalid_fd();
    test_fd_sync_directory();
    printf("All tests passed!\n");
    return 0;
}
