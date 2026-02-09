#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

static void write_all(int fd, const void *buf, size_t len)
{
    const uint8_t *cursor = (const uint8_t *)buf;
    size_t remaining = len;

    while (remaining > 0) {
        ssize_t written = write(fd, cursor, remaining);
        assert(written > 0);
        cursor += (size_t)written;
        remaining -= (size_t)written;
    }
}

static void read_all(int fd, void *buf, size_t len)
{
    uint8_t *cursor = (uint8_t *)buf;
    size_t remaining = len;

    while (remaining > 0) {
        ssize_t read_bytes = read(fd, cursor, remaining);
        assert(read_bytes > 0);
        cursor += (size_t)read_bytes;
        remaining -= (size_t)read_bytes;
    }
}

static void test_append_and_clear_flags(void)
{
    // From wasmtime p1_fd_flags_set.rs.
    printf("Test 1: append flag toggling\n");
    int fd = open("fd_fdstat_set_flags_file", O_CREAT | O_TRUNC | O_RDWR | O_APPEND, 0644);
    assert(fd >= 0);

    uint8_t data[100];
    memset(data, 0, sizeof(data));
    write_all(fd, data, sizeof(data));

    assert(lseek(fd, 0, SEEK_SET) == 0);
    uint8_t buffer[100];
    read_all(fd, buffer, sizeof(buffer));
    assert(memcmp(data, buffer, sizeof(buffer)) == 0);

    memset(data, 1, sizeof(data));
    assert(lseek(fd, 0, SEEK_SET) == 0);
    write_all(fd, data, sizeof(data));

    assert(lseek(fd, 100, SEEK_SET) == 100);
    read_all(fd, buffer, sizeof(buffer));
    assert(memcmp(data, buffer, sizeof(buffer)) == 0);

    __wasi_errno_t err = __wasi_fd_fdstat_set_flags((__wasi_fd_t)fd, 0);
    assert(err == __WASI_ERRNO_SUCCESS);

    memset(data, 2, sizeof(data));
    assert(lseek(fd, 0, SEEK_SET) == 0);
    write_all(fd, data, sizeof(data));

    assert(lseek(fd, 0, SEEK_SET) == 0);
    read_all(fd, buffer, sizeof(buffer));
    assert(memcmp(data, buffer, sizeof(buffer)) == 0);

    off_t size = lseek(fd, 0, SEEK_END);
    assert(size == 200);

    close(fd);
    assert(unlink("fd_fdstat_set_flags_file") == 0);
}

static void test_dir_fd_flags_set(void)
{
    // From wasmtime p1_dir_fd_op_failures.rs.
    printf("Test 2: fd_fdstat_set_flags on directory\n");
    int fd = open(".", O_RDONLY);
    assert(fd >= 0);

    __wasi_errno_t err =
        __wasi_fd_fdstat_set_flags((__wasi_fd_t)fd, __WASI_FDFLAGS_NONBLOCK);
    assert(err == __WASI_ERRNO_BADF);

    close(fd);
}

int main(void)
{
    test_append_and_clear_flags();
    test_dir_fd_flags_set();
    printf("All tests passed!\n");
    return 0;
}
