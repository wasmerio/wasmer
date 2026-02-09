#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

static int create_file(const char *name)
{
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    return fd;
}

static void test_set_cloexec(void)
{
    // From LTP fcntl01.c and gVisor fcntl.cc: set FD_CLOEXEC.
    printf("Test 1: set CLOEXEC\n");
    int fd = create_file("fd_fdflags_set_file");

    __wasi_fdflagsext_t flags = 0;
    __wasi_errno_t ret = __wasi_fd_fdflags_get(fd, &flags);
    assert(ret == __WASI_ERRNO_SUCCESS);
    assert((flags & __WASI_FDFLAGSEXT_CLOEXEC) == 0);

    ret = __wasi_fd_fdflags_set(fd, __WASI_FDFLAGSEXT_CLOEXEC);
    assert(ret == __WASI_ERRNO_SUCCESS);

    flags = 0;
    ret = __wasi_fd_fdflags_get(fd, &flags);
    assert(ret == __WASI_ERRNO_SUCCESS);
    assert((flags & __WASI_FDFLAGSEXT_CLOEXEC) != 0);

    assert(close(fd) == 0);
    assert(unlink("fd_fdflags_set_file") == 0);
}

static void test_clear_cloexec(void)
{
    // From gVisor fcntl.cc: clear FD_CLOEXEC.
    printf("Test 2: clear CLOEXEC\n");
    int fd = create_file("fd_fdflags_clear_file");

    __wasi_errno_t ret = __wasi_fd_fdflags_set(fd, __WASI_FDFLAGSEXT_CLOEXEC);
    assert(ret == __WASI_ERRNO_SUCCESS);

    ret = __wasi_fd_fdflags_set(fd, 0);
    assert(ret == __WASI_ERRNO_SUCCESS);

    __wasi_fdflagsext_t flags = __WASI_FDFLAGSEXT_CLOEXEC;
    ret = __wasi_fd_fdflags_get(fd, &flags);
    assert(ret == __WASI_ERRNO_SUCCESS);
    assert((flags & __WASI_FDFLAGSEXT_CLOEXEC) == 0);

    assert(close(fd) == 0);
    assert(unlink("fd_fdflags_clear_file") == 0);
}

static void test_independent_flags(void)
{
    // From gVisor fcntl.cc: descriptor flags are per-fd.
    printf("Test 3: independent descriptor flags\n");
    int fd = create_file("fd_fdflags_independent_file");
    int dup_fd = fcntl(fd, F_DUPFD, 0);
    assert(dup_fd >= 0);
    assert(dup_fd != fd);

    __wasi_errno_t ret = __wasi_fd_fdflags_set(fd, __WASI_FDFLAGSEXT_CLOEXEC);
    assert(ret == __WASI_ERRNO_SUCCESS);

    __wasi_fdflagsext_t flags = 0;
    ret = __wasi_fd_fdflags_get(dup_fd, &flags);
    assert(ret == __WASI_ERRNO_SUCCESS);
    assert((flags & __WASI_FDFLAGSEXT_CLOEXEC) == 0);

    ret = __wasi_fd_fdflags_set(dup_fd, __WASI_FDFLAGSEXT_CLOEXEC);
    assert(ret == __WASI_ERRNO_SUCCESS);

    flags = 0;
    ret = __wasi_fd_fdflags_get(fd, &flags);
    assert(ret == __WASI_ERRNO_SUCCESS);
    assert((flags & __WASI_FDFLAGSEXT_CLOEXEC) != 0);

    assert(close(dup_fd) == 0);
    assert(close(fd) == 0);
    assert(unlink("fd_fdflags_independent_file") == 0);
}

static void test_bad_fd(void)
{
    // From gVisor fcntl.cc: EBADF on closed fd.
    printf("Test 4: bad fd\n");
    int fd = create_file("fd_fdflags_bad_fd");
    assert(close(fd) == 0);

    __wasi_errno_t ret = __wasi_fd_fdflags_set(fd, __WASI_FDFLAGSEXT_CLOEXEC);
    assert(ret == __WASI_ERRNO_BADF);

    assert(unlink("fd_fdflags_bad_fd") == 0);
}

int main(void)
{
    test_set_cloexec();
    test_clear_cloexec();
    test_independent_flags();
    test_bad_fd();
    printf("All tests passed!\n");
    return 0;
}
