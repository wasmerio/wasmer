#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <wasi/api_wasix.h>

static void test_basic_pipe(void)
{
    printf("Test 1: pipe basic read/write\n");
    int fds[2] = {-1, -1};
    assert(pipe(fds) == 0);
    assert(fds[0] >= 0);
    assert(fds[1] >= 0);
    assert(fds[0] != fds[1]);

    const char msg[] = "abc";
    char buf[4] = {0};
    assert(write(fds[1], msg, sizeof(msg) - 1) == (ssize_t)(sizeof(msg) - 1));
    assert(read(fds[0], buf, sizeof(msg) - 1) == (ssize_t)(sizeof(msg) - 1));
    assert(memcmp(buf, msg, sizeof(msg) - 1) == 0);

    close(fds[0]);
    close(fds[1]);
}

static void test_wrong_end(void)
{
    printf("Test 2: wrong-end operations return EBADF\n");
    int fds[2] = {-1, -1};
    assert(pipe(fds) == 0);

    errno = 0;
    ssize_t wrc = write(fds[0], "x", 1);
    if (wrc != -1 || errno != EBADF) {
        fprintf(stderr, "write(read_end) rc=%zd errno=%d\n", wrc, errno);
    }
    assert(wrc == -1);
    assert(errno == EBADF);

    errno = 0;
    char c = 0;
    ssize_t rrc = read(fds[1], &c, 1);
    if (rrc != -1 || errno != EBADF) {
        fprintf(stderr, "read(write_end) rc=%zd errno=%d\n", rrc, errno);
    }
    assert(rrc == -1);
    assert(errno == EBADF);

    close(fds[0]);
    close(fds[1]);
}

static void test_eof_when_writer_closed(void)
{
    printf("Test 3: EOF after writer closed\n");
    int fds[2] = {-1, -1};
    assert(pipe(fds) == 0);

    close(fds[1]);
    char c = 'x';
    ssize_t n = read(fds[0], &c, 1);
    assert(n == 0);

    close(fds[0]);
}

static void test_invalid_pointer(void)
{
    printf("Test 4: invalid pointer -> MEMVIOLATION\n");
    __wasi_fd_t *bad = (__wasi_fd_t *)(uintptr_t)0xFFFFFFFCu;
    __wasi_errno_t err = __wasi_fd_pipe(bad, bad);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    printf("WASIX fd_pipe integration tests\n");
    test_basic_pipe();
    test_wrong_end();
    test_eof_when_writer_closed();
    test_invalid_pointer();
    printf("All tests passed!\n");
    return 0;
}
