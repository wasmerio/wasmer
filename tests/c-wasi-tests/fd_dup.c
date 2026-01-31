#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api_wasi.h>

static int create_file(const char *name)
{
    unlink(name);
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    return fd;
}

static void test_dup_read_write(void)
{
    printf("Test 1: fd_dup read/write via duplicate\n");
    const char payload[] = "Hello, dup!";
    char buf[sizeof(payload)] = {0};

    int fd = create_file("fd_dup_rw");

    __wasi_fd_t dup_fd = 0;
    __wasi_errno_t err = __wasi_fd_dup((__wasi_fd_t)fd, &dup_fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert((int)dup_fd != fd);

    // Write via duplicate, read via original.
    assert(write((int)dup_fd, payload, sizeof(payload)) == (ssize_t)sizeof(payload));
    assert(lseek(fd, 0, SEEK_SET) == 0);
    assert(read(fd, buf, sizeof(buf)) == (ssize_t)sizeof(buf));
    assert(memcmp(buf, payload, sizeof(payload)) == 0);

    // Closing original should not invalidate duplicate.
    assert(close(fd) == 0);
    assert(lseek((int)dup_fd, 0, SEEK_SET) == 0);
    memset(buf, 0, sizeof(buf));
    assert(read((int)dup_fd, buf, sizeof(buf)) == (ssize_t)sizeof(buf));
    assert(memcmp(buf, payload, sizeof(payload)) == 0);

    assert(close((int)dup_fd) == 0);
    assert(unlink("fd_dup_rw") == 0);
}

static void test_dup_bad_fd(void)
{
    printf("Test 2: fd_dup invalid fd (EBADF)\n");
    __wasi_fd_t dup_fd = 0;
    __wasi_errno_t err = __wasi_fd_dup((__wasi_fd_t)9999, &dup_fd);
    assert(err == __WASI_ERRNO_BADF);
}

int main(void)
{
    test_dup_read_write();
    test_dup_bad_fd();
    printf("All tests passed!\n");
    return 0;
}
