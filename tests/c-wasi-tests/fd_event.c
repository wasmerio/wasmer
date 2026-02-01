#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdio.h>
#include <string.h>
#include <sys/eventfd.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

static int failures = 0;

static void check(int cond, const char *msg)
{
    if (!cond) {
        fprintf(stderr, "FAIL: %s\n", msg);
        failures++;
    }
}

static void check_errno(int rc, int expected, const char *msg)
{
    if (rc != -1 || errno != expected) {
        fprintf(stderr, "FAIL: %s (rc=%d errno=%d expected=%d)\n",
                msg,
                rc,
                errno,
                expected);
        failures++;
    }
}

static void set_nonblock(int fd)
{
    __wasi_errno_t err = __wasi_fd_fdstat_set_flags((__wasi_fd_t)fd, __WASI_FDFLAGS_NONBLOCK);
    check(err == __WASI_ERRNO_SUCCESS, "fd_fdstat_set_flags(NONBLOCK) should succeed");
}

static void test_basic_read_write(void)
{
    printf("Test 1: eventfd basic read/write\n");
    int fd = eventfd(10, 0);
    check(fd >= 0, "eventfd should succeed");

    uint64_t val = 0;
    ssize_t rc = read(fd, &val, sizeof(val));
    check(rc == (ssize_t)sizeof(val), "read should return 8 bytes");
    check(val == 10, "read should return initial counter value");

    val = 5;
    rc = write(fd, &val, sizeof(val));
    check(rc == (ssize_t)sizeof(val), "write should accept u64");

    val = 0;
    rc = read(fd, &val, sizeof(val));
    check(rc == (ssize_t)sizeof(val), "read after write should return 8 bytes");
    check(val == 5, "read should return written value");

    close(fd);
}

static void test_nonblock_empty_read(void)
{
    printf("Test 2: eventfd nonblock empty read\n");
    int fd = eventfd(0, EFD_NONBLOCK);
    check(fd >= 0, "eventfd(EFD_NONBLOCK) should succeed");

    __wasi_fdstat_t stat;
    __wasi_errno_t err = __wasi_fd_fdstat_get((__wasi_fd_t)fd, &stat);
    check(err == __WASI_ERRNO_SUCCESS, "fd_fdstat_get should succeed");
    check((stat.fs_flags & __WASI_FDFLAGS_NONBLOCK) != 0,
          "EFD_NONBLOCK should set NONBLOCK fd flag");

    set_nonblock(fd);

    uint64_t val = 0;
    errno = 0;
    check_errno(read(fd, &val, sizeof(val)), EAGAIN, "read on empty nonblock should be EAGAIN");

    close(fd);
}

static void test_invalid_sizes_and_values(void)
{
    printf("Test 3: eventfd invalid sizes/values\n");
    int fd = eventfd(0, 0);
    check(fd >= 0, "eventfd should succeed");
    set_nonblock(fd);

    uint64_t val = 12;
    ssize_t rc = write(fd, &val, sizeof(val));
    check(rc == (ssize_t)sizeof(val), "write should succeed");

    uint64_t out = 0;
    rc = read(fd, &out, sizeof(out));
    check(rc == (ssize_t)sizeof(out), "read should succeed");
    check(out == 12, "read should return written value");

    uint32_t small = 0;
    val = 1;
    rc = write(fd, &val, sizeof(val));
    check(rc == (ssize_t)sizeof(val), "write before short read should succeed");
    errno = 0;
    check_errno(read(fd, &small, sizeof(small)), EINVAL,
                "short read should be EINVAL");

    errno = 0;
    check_errno(write(fd, &small, sizeof(small)), EINVAL,
                "short write should be EINVAL");

    val = UINT64_MAX;
    errno = 0;
    check_errno(write(fd, &val, sizeof(val)), EINVAL,
                "write(UINT64_MAX) should be EINVAL");

    val = UINT64_MAX - 1;
    rc = write(fd, &val, sizeof(val));
    check(rc == (ssize_t)sizeof(val), "write(UINT64_MAX-1) should succeed");

    val = 1;
    errno = 0;
    check_errno(write(fd, &val, sizeof(val)), EAGAIN,
                "write overflow should be EAGAIN on nonblock");

    close(fd);
}

static void test_semaphore_mode(void)
{
    printf("Test 4: eventfd semaphore mode\n");
    int fd = eventfd(0, EFD_SEMAPHORE);
    check(fd >= 0, "eventfd(EFD_SEMAPHORE) should succeed");
    set_nonblock(fd);

    uint64_t val = 2;
    ssize_t rc = write(fd, &val, sizeof(val));
    check(rc == (ssize_t)sizeof(val), "write(2) should succeed");

    uint64_t out = 0;
    rc = read(fd, &out, sizeof(out));
    check(rc == (ssize_t)sizeof(out), "read in semaphore mode should succeed");
    check(out == 1, "semaphore read should return 1");

    out = 0;
    rc = read(fd, &out, sizeof(out));
    check(rc == (ssize_t)sizeof(out), "second semaphore read should succeed");
    check(out == 1, "second semaphore read should return 1");

    errno = 0;
    check_errno(read(fd, &out, sizeof(out)), EAGAIN,
                "semaphore read on empty should be EAGAIN");

    close(fd);
}

static void test_invalid_flags(void)
{
    printf("Test 5: eventfd invalid flags\n");
    errno = 0;
    int fd = eventfd(0, ~0);
    check(fd == -1, "eventfd(~0) should fail");
    check(errno == EINVAL, "eventfd(~0) should set EINVAL");
}

static void test_illegal_seek(void)
{
    printf("Test 6: eventfd illegal seek\n");
    int fd = eventfd(0, 0);
    check(fd >= 0, "eventfd should succeed");

    errno = 0;
    check_errno(lseek(fd, 0, SEEK_SET), ESPIPE, "lseek on eventfd should be ESPIPE");

    close(fd);
}

int main(void)
{
    test_basic_read_write();
    test_nonblock_empty_read();
    test_invalid_sizes_and_values();
    test_semaphore_mode();
    test_invalid_flags();
    test_illegal_seek();

    assert(failures == 0);
    printf("All tests passed!\n");
    return 0;
}
