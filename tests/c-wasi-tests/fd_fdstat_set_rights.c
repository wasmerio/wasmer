#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <unistd.h>

#include <wasi/api_wasi.h>

static int create_file(const char *name)
{
    unlink(name);
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(write(fd, "hello", 5) == 5);
    assert(lseek(fd, 0, SEEK_SET) == 0);
    return fd;
}

static void test_bad_fd(void)
{
    // From wasmtime p2_adapter_badfd.rs: EBADF on invalid fd.
    printf("Test 1: bad fd\n");
    __wasi_errno_t err = __wasi_fd_fdstat_set_rights(9999, 0, 0);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_drop_read_rights(void)
{
    printf("Test 2: drop read rights\n");
    int fd = create_file("fd_fdstat_set_rights_file");

    __wasi_fdstat_t stat;
    __wasi_errno_t err = __wasi_fd_fdstat_get(fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert((stat.fs_rights_base & (__wasi_rights_t)__WASI_RIGHTS_FD_READ) != 0);

    __wasi_rights_t orig_base = stat.fs_rights_base;
    __wasi_rights_t orig_inherit = stat.fs_rights_inheriting;
    __wasi_rights_t new_base = orig_base & ~(__wasi_rights_t)__WASI_RIGHTS_FD_READ;

    err = __wasi_fd_fdstat_set_rights(fd, new_base, orig_inherit);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_fd_fdstat_get(fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.fs_rights_base == new_base);
    assert(stat.fs_rights_inheriting == orig_inherit);

    char buffer[4] = {0};
    __wasi_iovec_t iov = {.buf = (uint8_t *)buffer, .buf_len = sizeof(buffer)};
    __wasi_size_t nread = 0;
    err = __wasi_fd_read(fd, &iov, 1, &nread);
    assert(err == __WASI_ERRNO_ACCES);

    err = __wasi_fd_fdstat_set_rights(fd, orig_base, orig_inherit);
    assert(err == __WASI_ERRNO_NOTCAPABLE);

    assert(close(fd) == 0);
    assert(unlink("fd_fdstat_set_rights_file") == 0);
}

int main(void)
{
    test_bad_fd();
    test_drop_read_rights();
    printf("All tests passed!\n");
    return 0;
}
