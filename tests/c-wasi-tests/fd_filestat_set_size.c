#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/socket.h>
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

static void test_set_size_shrink_and_extend(void)
{
    // From LTP ftruncate01.c: shrink and extend with data validation.
    printf("Test 4: shrink and extend preserves/zeros data\n");
    int fd = create_file_rw("fd_filestat_set_size_data");

    // Fill file with 'a' bytes.
    const size_t initial = 1024;
    char fill[initial];
    memset(fill, 'a', sizeof(fill));
    assert(write(fd, fill, sizeof(fill)) == (ssize_t)sizeof(fill));

    __wasi_errno_t err = __wasi_fd_filestat_set_size(fd, 256);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(file_size(fd) == 256);

    // Verify first 256 bytes are still 'a'.
    assert(lseek(fd, 0, SEEK_SET) == 0);
    char buf[256];
    assert(read(fd, buf, sizeof(buf)) == (ssize_t)sizeof(buf));
    for (size_t i = 0; i < sizeof(buf); i++) {
        assert(buf[i] == 'a');
    }

    // Extend to 512 and ensure new region is zero-filled.
    err = __wasi_fd_filestat_set_size(fd, 512);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(file_size(fd) == 512);

    assert(lseek(fd, 256, SEEK_SET) == 256);
    char zeros[256];
    assert(read(fd, zeros, sizeof(zeros)) == (ssize_t)sizeof(zeros));
    for (size_t i = 0; i < sizeof(zeros); i++) {
        assert(zeros[i] == 0);
    }

    assert(close(fd) == 0);
    assert(unlink("fd_filestat_set_size_data") == 0);
}

static void test_set_size_invalid_fd(void)
{
    printf("Test 5: set size on invalid fd\n");
    __wasi_errno_t err = __wasi_fd_filestat_set_size((__wasi_fd_t)9999, 1);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_set_size_socket(void)
{
    printf("Test 6: set size on socket fd\n");
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_filestat_set_size(fd, 1);
    assert(err == __WASI_ERRNO_BADF);

    assert(close(fd) == 0);
}

static void test_set_size_pipe(void)
{
    printf("Test 7: set size on pipe fd\n");
    int pipefd[2];
    assert(pipe(pipefd) == 0);

    __wasi_errno_t err = __wasi_fd_filestat_set_size(pipefd[0], 1);
    assert(err == __WASI_ERRNO_BADF);

    assert(close(pipefd[0]) == 0);
    assert(close(pipefd[1]) == 0);
}

int main(void)
{
    test_set_size_rw();
    test_set_size_ro();
    test_set_size_directory();
    test_set_size_shrink_and_extend();
    test_set_size_invalid_fd();
    test_set_size_socket();
    test_set_size_pipe();
    printf("All tests passed!\n");
    return 0;
}
