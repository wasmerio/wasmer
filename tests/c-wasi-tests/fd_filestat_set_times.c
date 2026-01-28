#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <unistd.h>

#include <wasi/api_wasi.h>

static void assert_time_close(uint64_t actual, uint64_t expected, uint64_t tolerance)
{
    if (actual >= expected) {
        assert(actual - expected <= tolerance);
    } else {
        assert(expected - actual <= tolerance);
    }
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

static void test_set_times_rw(void)
{
    // From wasmtime p1_fd_filestat_set.rs: set mtim, atim unchanged.
    printf("Test 1: set mtim on read/write file\n");
    int fd = create_file_rw("fd_filestat_set_times_rw");

    __wasi_filestat_t stat;
    __wasi_errno_t err = __wasi_fd_filestat_get(fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint64_t old_atim = stat.atim;
    uint64_t old_mtim = stat.mtim;
    uint64_t delta = 2000000000ULL;
    uint64_t new_mtim = (old_mtim > delta) ? (old_mtim - delta) : (old_mtim + delta);

    err = __wasi_fd_filestat_set_times(fd, new_mtim, new_mtim, __WASI_FSTFLAGS_MTIM);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_fd_filestat_get(fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.size == 0);
    assert_time_close(stat.mtim, new_mtim, 1000000000ULL);
    assert_time_close(stat.atim, old_atim, 1000000000ULL);

    assert(close(fd) == 0);
    assert(unlink("fd_filestat_set_times_rw") == 0);
}

static void test_set_times_ro(void)
{
    // From wasmtime p1_fd_filestat_set.rs: read-only open should still allow set_times.
    printf("Test 2: set mtim on read-only file\n");
    int fd = create_file_rw("fd_filestat_set_times_ro");
    assert(close(fd) == 0);

    fd = create_file_ro("fd_filestat_set_times_ro");

    __wasi_filestat_t stat;
    __wasi_errno_t err = __wasi_fd_filestat_get(fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint64_t old_mtim = stat.mtim;
    uint64_t delta = 2000000000ULL;
    uint64_t new_mtim = (old_mtim > delta) ? (old_mtim - delta) : (old_mtim + delta);

    err = __wasi_fd_filestat_set_times(fd, new_mtim, new_mtim, __WASI_FSTFLAGS_MTIM);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_fd_filestat_get(fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_time_close(stat.mtim, new_mtim, 1000000000ULL);

    assert(close(fd) == 0);
    assert(unlink("fd_filestat_set_times_ro") == 0);
}

static void test_invalid_flags(void)
{
    // From wasmtime p1_path_filestat.rs: invalid flag combinations.
    printf("Test 3: invalid fst_flags combinations\n");
    int fd = create_file_rw("fd_filestat_set_times_flags");

    __wasi_errno_t err = __wasi_fd_filestat_set_times(
        fd,
        0,
        0,
        (__wasi_fstflags_t)(__WASI_FSTFLAGS_ATIM | __WASI_FSTFLAGS_ATIM_NOW));
    assert(err == __WASI_ERRNO_INVAL);

    err = __wasi_fd_filestat_set_times(
        fd,
        0,
        0,
        (__wasi_fstflags_t)(__WASI_FSTFLAGS_MTIM | __WASI_FSTFLAGS_MTIM_NOW));
    assert(err == __WASI_ERRNO_INVAL);

    assert(close(fd) == 0);
    assert(unlink("fd_filestat_set_times_flags") == 0);
}

int main(void)
{
    test_set_times_rw();
    test_set_times_ro();
    test_invalid_flags();
    printf("All tests passed!\n");
    return 0;
}
