#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

static off_t file_size(int fd)
{
    struct stat st;
    assert(fstat(fd, &st) == 0);
    return st.st_size;
}

static void test_basic_growth_and_no_shrink(void)
{
    // From gVisor fallocate.cc: growth, no-shrink, and offset growth.
    printf("Test 1: allocate grows file and does not shrink\n");
    unlink("fd_allocate_basic");
    int fd = open("fd_allocate_basic", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(file_size(fd) == 0);

    __wasi_errno_t err = __wasi_fd_allocate((__wasi_fd_t)fd, 0, 10);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(file_size(fd) == 10);

    err = __wasi_fd_allocate((__wasi_fd_t)fd, 0, 5);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(file_size(fd) == 10);

    err = __wasi_fd_allocate((__wasi_fd_t)fd, 0, 20);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(file_size(fd) == 20);

    err = __wasi_fd_allocate((__wasi_fd_t)fd, 10, 20);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(file_size(fd) == 30);

    err = __wasi_fd_allocate((__wasi_fd_t)fd, 39, 1);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(file_size(fd) == 40);

    close(fd);
    assert(unlink("fd_allocate_basic") == 0);
}

static void test_len_zero_invalid(void)
{
    // From gVisor fallocate.cc: length 0 should be invalid.
    printf("Test 2: zero length is invalid\n");
    unlink("fd_allocate_len0");
    int fd = open("fd_allocate_len0", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_allocate((__wasi_fd_t)fd, 0, 0);
    assert(err == __WASI_ERRNO_INVAL);
    assert(file_size(fd) == 0);

    close(fd);
    assert(unlink("fd_allocate_len0") == 0);
}

static void test_invalid_fd(void)
{
    // From LTP fallocate02.c and gVisor fallocate.cc.
    printf("Test 3: invalid fd\n");
    __wasi_errno_t err = __wasi_fd_allocate((__wasi_fd_t)9999, 0, 10);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_directory_fd(void)
{
    // From wasmtime p1_dir_fd_op_failures.rs.
    printf("Test 4: directory fd\n");
    int fd = open(".", O_RDONLY);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_allocate((__wasi_fd_t)fd, 0, 1);
    assert(err == __WASI_ERRNO_BADF);

    close(fd);
}

static void test_missing_rights(void)
{
    // From LTP fallocate02.c (read-only/permission failure).
    printf("Test 5: missing FD_ALLOCATE rights\n");
    unlink("fd_allocate_rights");
    int fd = open("fd_allocate_rights", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_fdstat_t stat;
    __wasi_errno_t err = __wasi_fd_fdstat_get((__wasi_fd_t)fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_rights_t new_base = stat.fs_rights_base & ~(__wasi_rights_t)__WASI_RIGHTS_FD_ALLOCATE;
    err = __wasi_fd_fdstat_set_rights((__wasi_fd_t)fd, new_base, stat.fs_rights_inheriting);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_fd_allocate((__wasi_fd_t)fd, 0, 10);
    assert(err == __WASI_ERRNO_ACCES);

    close(fd);
    assert(unlink("fd_allocate_rights") == 0);
}

int main(void)
{
    test_basic_growth_and_no_shrink();
    test_len_zero_invalid();
    test_invalid_fd();
    test_directory_fd();
    test_missing_rights();
    printf("All tests passed!\n");
    return 0;
}
