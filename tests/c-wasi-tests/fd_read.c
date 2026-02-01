#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api_wasi.h>

static int create_file_with(const char *name, const void *data, size_t len)
{
    unlink(name);
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    if (len > 0) {
        assert(write(fd, data, len) == (ssize_t)len);
    }
    assert(lseek(fd, 0, SEEK_SET) == 0);
    return fd;
}

static void log_test(const char *msg)
{
    printf("%s\n", msg);
    fflush(stdout);
}

static void test_basic_read(void)
{
    // From LTP read01/read04: read returns expected count and data.
    log_test("Test 1: basic read count + data");
    const char payload[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    int fd = create_file_with("fd_read_basic", payload, strlen(payload));

    char buf[64] = {0};
    ssize_t n = read(fd, buf, sizeof(buf));
    assert(n == (ssize_t)strlen(payload));
    assert(memcmp(buf, payload, strlen(payload)) == 0);

    assert(close(fd) == 0);
    assert(unlink("fd_read_basic") == 0);
}

static void test_empty_read_returns_zero(void)
{
    // From wasmtime p1_path_open_read_write.rs: empty file read returns 0.
    log_test("Test 2: empty file read returns 0");
    int fd = create_file_with("fd_read_empty", NULL, 0);
    char buf[4] = {1, 1, 1, 1};
    ssize_t n = read(fd, buf, sizeof(buf));
    assert(n == 0);
    assert(close(fd) == 0);
    assert(unlink("fd_read_empty") == 0);
}

static void test_read_writeonly_fd(void)
{
    // From gVisor open.cc: read on write-only fd fails with EBADF.
    log_test("Test 3: read on write-only fd -> EBADF");
    int fd = create_file_with("fd_read_wo", "X", 1);
    int wfd = open("fd_read_wo", O_WRONLY);
    assert(wfd >= 0);
    char c = 0;
    errno = 0;
    ssize_t n = read(wfd, &c, 1);
    if (n != -1 || errno != EBADF) {
        fprintf(stderr, "Expected EBADF, got n=%zd errno=%d\n", n, errno);
        assert(0);
    }
    assert(close(wfd) == 0);
    assert(close(fd) == 0);
    assert(unlink("fd_read_wo") == 0);
}

static void test_read_directory(void)
{
    // From LTP read02: read on a directory returns EISDIR.
    log_test("Test 4: read on directory -> EISDIR");
    int fd = open(".", O_RDONLY);
    assert(fd >= 0);
    char buf[8];
    errno = 0;
    ssize_t n = read(fd, buf, sizeof(buf));
    if (n != -1 || errno != EISDIR) {
        fprintf(stderr, "Expected EISDIR, got n=%zd errno=%d\n", n, errno);
        assert(0);
    }
    assert(close(fd) == 0);
}

static void test_read_bad_fd(void)
{
    // From LTP read02 and LLVM libc read_write_test.cpp: EBADF on invalid fd.
    log_test("Test 5: read invalid fd -> EBADF");
    char c = 0;
    errno = 0;
    ssize_t n = read(-1, &c, 1);
    if (n != -1 || errno != EBADF) {
        fprintf(stderr, "Expected EBADF, got n=%zd errno=%d\n", n, errno);
        assert(0);
    }
}

static void test_read_invalid_buffer(void)
{
    // From LTP read02 + LLVM libc read_write_test.cpp: EFAULT on bad buffer.
    log_test("Test 6: read invalid buffer -> EFAULT");
    int fd = create_file_with("fd_read_fault", "abc", 3);

    __wasi_iovec_t iov;
    iov.buf = (uint8_t *)0xFFFFF000u;
    iov.buf_len = 4;
    __wasi_size_t nread = 123;
    __wasi_errno_t err = __wasi_fd_read(fd, &iov, 1, &nread);
    if (err != __WASI_ERRNO_FAULT) {
        fprintf(stderr, "Expected __WASI_ERRNO_FAULT, got %u\n", (unsigned)err);
        assert(0);
    }

    assert(close(fd) == 0);
    assert(unlink("fd_read_fault") == 0);
}

static void test_pipe_nonblock_eagain(void)
{
    // From LTP read03 and gVisor pipe.cc: nonblocking read on empty pipe -> EAGAIN.
    log_test("Test 7: nonblocking pipe read empty -> EAGAIN/EWOULDBLOCK");
    int fds[2];
    assert(pipe(fds) == 0);
    int flags = fcntl(fds[0], F_GETFL);
    assert(flags >= 0);
    assert(fcntl(fds[0], F_SETFL, flags | O_NONBLOCK) == 0);
    char c = 0;
    errno = 0;
    ssize_t n = read(fds[0], &c, 1);
    if (n != -1 || !(errno == EAGAIN || errno == EWOULDBLOCK)) {
        fprintf(stderr, "Expected EAGAIN/EWOULDBLOCK, got n=%zd errno=%d\n", n, errno);
        assert(0);
    }
    assert(close(fds[0]) == 0);
    assert(close(fds[1]) == 0);
}

static void test_pipe_eof_returns_zero(void)
{
    // From gVisor pipe.cc: read returns 0 on EOF after write end closed.
    log_test("Test 8: pipe EOF read returns 0");
    int fds[2];
    assert(pipe(fds) == 0);
    assert(close(fds[1]) == 0);
    char c = 0;
    ssize_t n = read(fds[0], &c, 1);
    assert(n == 0);
    assert(close(fds[0]) == 0);
}

static void test_zero_length_read(void)
{
    // From gVisor pipe.cc: read(0) returns 0.
    log_test("Test 9: zero-length read returns 0");
    int fd = create_file_with("fd_read_zero", "abc", 3);
    ssize_t n = read(fd, NULL, 0);
    assert(n == 0);
    assert(close(fd) == 0);
    assert(unlink("fd_read_zero") == 0);
}

static void test_multi_iovec_read(void)
{
    // From wasmtime p1_file_read_write.rs: multiple iovecs read.
    log_test("Test 10: multi-iovec read");
    uint8_t payload[4] = {0, 1, 2, 3};
    int fd = create_file_with("fd_read_iovec", payload, sizeof(payload));

    uint8_t buf[4] = {0xff, 0xff, 0xff, 0xff};
    __wasi_iovec_t iov[2];
    iov[0].buf = &buf[0];
    iov[0].buf_len = 2;
    iov[1].buf = &buf[2];
    iov[1].buf_len = 2;

    __wasi_size_t nread = 0;
    __wasi_errno_t err = __wasi_fd_read(fd, iov, 2, &nread);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nread == 4);
    assert(memcmp(buf, payload, sizeof(payload)) == 0);

    assert(close(fd) == 0);
    assert(unlink("fd_read_iovec") == 0);
}

static void test_seek_then_read(void)
{
    // From wasmtime p1_file_read_write.rs: read after seek returns tail data.
    log_test("Test 11: seek then read");
    const char payload[] = "abcd";
    int fd = create_file_with("fd_read_seek", payload, strlen(payload));

    assert(lseek(fd, 2, SEEK_SET) == 2);
    char buf[4] = {0};
    ssize_t n = read(fd, buf, sizeof(buf));
    assert(n == 2);
    assert(memcmp(buf, "cd", 2) == 0);

    assert(close(fd) == 0);
    assert(unlink("fd_read_seek") == 0);
}

int main(void)
{
    test_basic_read();
    test_empty_read_returns_zero();
    test_read_directory();
    test_read_bad_fd();
    test_pipe_nonblock_eagain();
    test_pipe_eof_returns_zero();
    test_zero_length_read();
    test_multi_iovec_read();
    test_seek_then_read();
    test_read_invalid_buffer();
    test_read_writeonly_fd();
    printf("All tests passed!\n");
    return 0;
}
