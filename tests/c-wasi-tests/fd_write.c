#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include <wasi/api_wasi.h>

static int create_file(const char *name)
{
    unlink(name);
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    return fd;
}

static void log_test(const char *msg)
{
    printf("%s\n", msg);
    fflush(stdout);
}

static int failures = 0;
#define EXPECT(cond, fmt, ...)                                                        \
    do {                                                                              \
        if (!(cond)) {                                                                \
            fprintf(stderr, "FAIL: " fmt "\n", ##__VA_ARGS__);                         \
            failures++;                                                               \
        }                                                                             \
    } while (0)

static void test_basic_write_counts(void)
{
    // From LTP write01.c: write returns requested count.
    log_test("Test 1: basic write counts");
    int fd = create_file("fd_write_basic");
    char buf[BUFSIZ];
    memset(buf, 'w', sizeof(buf));

    for (int i = BUFSIZ; i > 0; i--) {
        ssize_t n = write(fd, buf, i);
        assert(n == i);
    }

    assert(close(fd) == 0);
    assert(unlink("fd_write_basic") == 0);
}

static void test_write_null_zero(void)
{
    // From LTP write02.c: write(NULL, 0) returns 0.
    log_test("Test 2: write NULL,0 returns 0");
    int fd = create_file("fd_write_zero");
    ssize_t n = write(fd, NULL, 0);
    assert(n == 0);
    assert(close(fd) == 0);
    assert(unlink("fd_write_zero") == 0);
}

static void test_write_increments_offset(void)
{
    // From gVisor write.cc: write advances file offset.
    log_test("Test 3: write increments offset");
    int fd = create_file("fd_write_offset");

    assert(lseek(fd, 0, SEEK_CUR) == 0);
    assert(write(fd, "abc", 3) == 3);
    assert(lseek(fd, 0, SEEK_CUR) == 3);

    assert(close(fd) == 0);
    assert(unlink("fd_write_offset") == 0);
}

static void test_write_append(void)
{
    // From LTP write06.c and gVisor write.cc: O_APPEND writes at end.
    log_test("Test 4: O_APPEND writes at end");
    const size_t k1 = 1024;
    const size_t k2 = 2048;
    const size_t k3 = 3072;
    char buf2[2048];
    char buf1[1024];
    memset(buf2, 0, sizeof(buf2));
    memset(buf1, 1, sizeof(buf1));

    int fd = create_file("fd_write_append");
    assert(write(fd, buf2, sizeof(buf2)) == (ssize_t)sizeof(buf2));
    assert(close(fd) == 0);

    fd = open("fd_write_append", O_RDWR | O_APPEND);
    assert(fd >= 0);

    assert(lseek(fd, k1, SEEK_SET) == (off_t)k1);
    assert(write(fd, buf1, sizeof(buf1)) == (ssize_t)sizeof(buf1));
    assert(lseek(fd, 0, SEEK_CUR) == (off_t)k3);

    struct stat st;
    assert(fstat(fd, &st) == 0);
    assert((size_t)st.st_size == k3);

    assert(close(fd) == 0);
    assert(unlink("fd_write_append") == 0);
}

static void test_write_pipe_read_end_badf(void)
{
    // From gVisor pipe.cc: writing to read end -> EBADF.
    log_test("Test 5: write to pipe read end -> EBADF");
    int fds[2];
    assert(pipe(fds) == 0);
    char c = 'x';
    errno = 0;
    ssize_t n = write(fds[0], &c, 1);
    EXPECT(n == -1 && errno == EBADF, "pipe read end write expected EBADF, got n=%zd errno=%d", n,
           errno);
    assert(close(fds[0]) == 0);
    assert(close(fds[1]) == 0);
}

static void test_write_pipe_nonblock_eagain(void)
{
    // From LTP write04.c: nonblocking pipe full -> EAGAIN/EWOULDBLOCK.
    log_test("Test 6: nonblocking pipe full -> EAGAIN/EWOULDBLOCK");
    int fds[2];
    assert(pipe(fds) == 0);
    int flags = fcntl(fds[1], F_GETFL);
    assert(flags >= 0);
    assert(fcntl(fds[1], F_SETFL, flags | O_NONBLOCK) == 0);

    char buf[1024];
    memset(buf, 'p', sizeof(buf));
    int saw_eagain = 0;
    for (int i = 0; i < 1024; i++) {
        ssize_t n = write(fds[1], buf, sizeof(buf));
        if (n == -1) {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                saw_eagain = 1;
                break;
            }
            EXPECT(0, "pipe nonblock write unexpected errno=%d", errno);
            break;
        }
    }
    EXPECT(saw_eagain, "pipe nonblock write never returned EAGAIN/EWOULDBLOCK");

    assert(close(fds[0]) == 0);
    assert(close(fds[1]) == 0);
}

static void test_write_multi_iovec(void)
{
    // From wasmtime p1_file_read_write.rs: multi-iovec write.
    log_test("Test 7: multi-iovec write");
    int fd = create_file("fd_write_iovec");

    uint8_t a[2] = {0, 1};
    uint8_t b[2] = {2, 3};
    __wasi_ciovec_t iov[2];
    iov[0].buf = a;
    iov[0].buf_len = sizeof(a);
    iov[1].buf = b;
    iov[1].buf_len = sizeof(b);
    __wasi_size_t nw = 0;
    __wasi_errno_t err = __wasi_fd_write(fd, iov, 2, &nw);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nw == 4);

    assert(lseek(fd, 0, SEEK_SET) == 0);
    uint8_t out[4] = {0};
    assert(read(fd, out, sizeof(out)) == 4);
    assert(memcmp(out, (uint8_t[]){0, 1, 2, 3}, 4) == 0);

    assert(close(fd) == 0);
    assert(unlink("fd_write_iovec") == 0);
}

static void test_unbuffered_write_read_other_fd(void)
{
    // From wasmtime p1_file_unbuffered_write.rs: write visible to read fd.
    log_test("Test 8: write visible to separate read fd");
    int fd_read = open("fd_write_unbuffered", O_CREAT | O_TRUNC | O_RDONLY, 0644);
    if (fd_read < 0) {
        EXPECT(0, "open read-only failed errno=%d", errno);
        return;
    }
    int fd_write = open("fd_write_unbuffered", O_WRONLY);
    if (fd_write < 0) {
        EXPECT(0, "open write-only failed errno=%d", errno);
        assert(close(fd_read) == 0);
        return;
    }

    assert(write(fd_write, "Z", 1) == 1);
    assert(lseek(fd_read, 0, SEEK_SET) == 0);
    char c = 0;
    assert(read(fd_read, &c, 1) == 1);
    assert(c == 'Z');

    assert(close(fd_write) == 0);
    assert(close(fd_read) == 0);
    assert(unlink("fd_write_unbuffered") == 0);
}

static void test_large_write_size_and_contents(void)
{
    // From wasmtime p1_file_write.rs: long write, size, and readback slices.
    log_test("Test 9: large write size + readback");
    int fd = create_file("fd_write_large");

    const size_t total = 64 * 1024;
    char *buf = malloc(total);
    assert(buf);
    for (size_t i = 0; i < total; i++) {
        buf[i] = (char)('a' + (i % 26));
    }

    ssize_t nw = write(fd, buf, total);
    assert(nw == (ssize_t)total);

    struct stat st;
    assert(fstat(fd, &st) == 0);
    assert((size_t)st.st_size == total);

    assert(lseek(fd, 0, SEEK_SET) == 0);
    char head[32];
    assert(read(fd, head, sizeof(head)) == (ssize_t)sizeof(head));
    assert(memcmp(head, buf, sizeof(head)) == 0);

    assert(lseek(fd, (off_t)(total - sizeof(head)), SEEK_SET) == (off_t)(total - sizeof(head)));
    char tail[32];
    assert(read(fd, tail, sizeof(tail)) == (ssize_t)sizeof(tail));
    assert(memcmp(tail, buf + total - sizeof(tail), sizeof(tail)) == 0);

    free(buf);
    assert(close(fd) == 0);
    assert(unlink("fd_write_large") == 0);
}

static void test_write_readonly_fd(void)
{
    // From wasmtime p1_path_open_read_write.rs and gVisor open.cc.
    log_test("Test 10: write on read-only fd -> EBADF");
    int fd = create_file("fd_write_readonly");
    assert(close(fd) == 0);

    int ro = open("fd_write_readonly", O_RDONLY);
    assert(ro >= 0);
    errno = 0;
    ssize_t n = write(ro, "x", 1);
    EXPECT(n == -1 && errno == EBADF, "write on read-only expected EBADF, got n=%zd errno=%d", n,
           errno);
    assert(close(ro) == 0);
    assert(unlink("fd_write_readonly") == 0);
}

static void test_write_invalid_buffer_no_corruption(void)
{
    // From LTP write03/write05: EFAULT and no file corruption.
    log_test("Test 11: invalid buffer -> EFAULT and file unchanged");
    int fd = create_file("fd_write_fault");
    assert(write(fd, "AAAA", 4) == 4);
    assert(lseek(fd, 0, SEEK_SET) == 0);

    __wasi_ciovec_t iov;
    iov.buf = (uint8_t *)0xFFFFF000u;
    iov.buf_len = 4;
    __wasi_size_t nwritten = 0;
    __wasi_errno_t err = __wasi_fd_write(fd, &iov, 1, &nwritten);
    EXPECT(err == __WASI_ERRNO_FAULT, "invalid buffer expected EFAULT, got %u", (unsigned)err);

    assert(lseek(fd, 0, SEEK_SET) == 0);
    char out[4] = {0};
    assert(read(fd, out, sizeof(out)) == 4);
    assert(memcmp(out, "AAAA", 4) == 0);

    assert(close(fd) == 0);
    assert(unlink("fd_write_fault") == 0);
}

int main(void)
{
    test_basic_write_counts();
    test_write_null_zero();
    test_write_increments_offset();
    test_write_append();
    test_write_multi_iovec();
    test_large_write_size_and_contents();
    // Likely failures are kept last to allow other scenarios to run.
    test_write_pipe_read_end_badf();
    test_write_readonly_fd();
    test_write_invalid_buffer_no_corruption();
    test_write_pipe_nonblock_eagain();
    test_unbuffered_write_read_other_fd();
    if (failures != 0) {
        fprintf(stderr, "%d failure(s)\n", failures);
        assert(0);
    }
    printf("All tests passed!\n");
    return 0;
}
