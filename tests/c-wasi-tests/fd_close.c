#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/socket.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

static void test_close_regular_file(void)
{
    printf("Test 1: close regular file\n");
    __wasi_fdstat_t stat;

    int fd = open("fd_close_regular_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_fdstat_get((__wasi_fd_t)fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_fd_close((__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    err = __wasi_fd_fdstat_get((__wasi_fd_t)fd, &stat);
    assert(err == __WASI_ERRNO_BADF);
    assert(unlink("fd_close_regular_file") == 0);
}

static void test_close_already_closed(void)
{
    // From wasmtime p1_renumber.rs: closing a closed fd returns EBADF.
    printf("Test 2: close already closed\n");
    int fd = open("fd_close_again", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_close((__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_fd_close((__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_BADF);

    assert(unlink("fd_close_again") == 0);
}

static void test_close_invalid_fd(void)
{
    // From wasmtime p2_adapter_badfd.rs: invalid fd returns EBADF.
    printf("Test 3: close invalid fd\n");
    __wasi_errno_t err = __wasi_fd_close((__wasi_fd_t)9999);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_close_pipe(void)
{
    // From LTP close01.c: closing pipe fd succeeds.
    printf("Test 4: close pipe fd\n");
    int pipefd[2];
    assert(pipe(pipefd) == 0);

    __wasi_errno_t err = __wasi_fd_close((__wasi_fd_t)pipefd[0]);
    assert(err == __WASI_ERRNO_SUCCESS);
    err = __wasi_fd_close((__wasi_fd_t)pipefd[1]);
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_close_socket(void)
{
    // From LTP close01.c: closing socket fd succeeds.
    printf("Test 5: close socket fd\n");
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_close((__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_SUCCESS);
}

int main(void)
{
    test_close_regular_file();
    test_close_already_closed();
    test_close_invalid_fd();
    test_close_pipe();
    test_close_socket();
    printf("All tests passed!\n");
    return 0;
}
