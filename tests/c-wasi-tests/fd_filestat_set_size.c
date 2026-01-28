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

static int create_file_rw(const char *name)
{
    unlink(name);
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    return fd;
}

static int create_file_ro(const char *name)
{
    int fd = open(name, O_RDONLY);
    assert(fd >= 0);
    return fd;
}

static void test_set_size_rw(void)
{
    // From wasmtime p1_fd_filestat_set.rs: set size on RW file.
    printf("Test 1: set size on read/write file\n");
    int fd = create_file_rw("fd_filestat_set_size_rw");
    assert(file_size(fd) == 0);

    __wasi_errno_t err = __wasi_fd_filestat_set_size(fd, 100);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(file_size(fd) == 100);

    assert(close(fd) == 0);
    assert(unlink("fd_filestat_set_size_rw") == 0);
}

static void test_set_size_ro(void)
{
    // From wasmtime p1_fd_filestat_set.rs: read-only set_size should fail.
    printf("Test 2: set size on read-only file\n");
    int fd = create_file_rw("fd_filestat_set_size_ro");
    assert(close(fd) == 0);

    fd = create_file_ro("fd_filestat_set_size_ro");
    assert(file_size(fd) == 0);

    __wasi_errno_t err = __wasi_fd_filestat_set_size(fd, 100);
    assert(err == __WASI_ERRNO_INVAL || err == __WASI_ERRNO_ACCES);
    assert(file_size(fd) == 0);

    assert(close(fd) == 0);
    assert(unlink("fd_filestat_set_size_ro") == 0);
}

static void test_set_size_directory(void)
{
    // From wasmtime p1_dir_fd_op_failures.rs: fd_filestat_set_size on directory.
    printf("Test 3: set size on directory fd\n");
    int fd = open(".", O_RDONLY);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_filestat_set_size(fd, 0);
    assert(err == __WASI_ERRNO_BADF);

    assert(close(fd) == 0);
}

int main(void)
{
    test_set_size_rw();
    test_set_size_ro();
    test_set_size_directory();
    printf("All tests passed!\n");
    return 0;
}
