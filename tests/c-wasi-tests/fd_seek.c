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
    return fd;
}

static void test_basic_seek(void)
{
    // From wasmtime p1_file_seek_tell.rs: seek behavior and bounds.
    printf("Test 1: basic seek behavior\n");
    int fd = create_file("fd_seek_basic");

    __wasi_filesize_t pos = 0;
    __wasi_errno_t err = __wasi_fd_seek(fd, 0, __WASI_WHENCE_CUR, &pos);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pos == 0);

    char data[100] = {0};
    assert(write(fd, data, sizeof(data)) == (ssize_t)sizeof(data));

    err = __wasi_fd_seek(fd, -50, __WASI_WHENCE_CUR, &pos);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pos == 50);

    err = __wasi_fd_seek(fd, 0, __WASI_WHENCE_SET, &pos);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pos == 0);

    err = __wasi_fd_seek(fd, 1000, __WASI_WHENCE_CUR, &pos);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pos == 1000);

    err = __wasi_fd_seek(fd, -2000, __WASI_WHENCE_CUR, &pos);
    assert(err == __WASI_ERRNO_INVAL);

    assert(close(fd) == 0);
    assert(unlink("fd_seek_basic") == 0);
}

static void test_append_seek(void)
{
    // From wasmtime p1_file_seek_tell.rs: fd_seek with O_APPEND.
    printf("Test 2: seek with O_APPEND\n");
    unlink("fd_seek_append");
    int fd = open("fd_seek_append", O_CREAT | O_TRUNC | O_RDWR | O_APPEND, 0644);
    assert(fd >= 0);

    __wasi_filesize_t pos = 0;
    __wasi_errno_t err = __wasi_fd_seek(fd, 0, __WASI_WHENCE_CUR, &pos);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pos == 0);

    char data[100] = {0};
    assert(write(fd, data, sizeof(data)) == (ssize_t)sizeof(data));

    err = __wasi_fd_seek(fd, 0, __WASI_WHENCE_CUR, &pos);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pos == 100);

    assert(close(fd) == 0);
    assert(unlink("fd_seek_append") == 0);
}

static void test_directory_seek(void)
{
    // From wasmtime p1_directory_seek.rs and p1_dir_fd_op_failures.rs.
    printf("Test 3: seek on directory fd\n");
    int fd = open(".", O_RDONLY);
    assert(fd >= 0);

    __wasi_filesize_t pos = 0;
    __wasi_errno_t err = __wasi_fd_seek(fd, 0, __WASI_WHENCE_CUR, &pos);
    assert(err == __WASI_ERRNO_BADF);

    err = __wasi_fd_seek(fd, 0, __WASI_WHENCE_SET, &pos);
    assert(err == __WASI_ERRNO_BADF);

    err = __wasi_fd_seek(fd, 0, __WASI_WHENCE_END, &pos);
    assert(err == __WASI_ERRNO_BADF);

    assert(close(fd) == 0);
}

int main(void)
{
    test_basic_seek();
    test_append_seek();
    test_directory_seek();
    printf("All tests passed!\n");
    return 0;
}
