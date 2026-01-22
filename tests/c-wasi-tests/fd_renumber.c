#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

#define FILE1 "fd_renumber_file1"
#define FILE2 "fd_renumber_file2"
#define FILE3 "fd_renumber_file3"

static int find_preopen_fd(void)
{
    // fd 3 is VIRTUAL_ROOT_FD in Wasmer; pick a real preopen instead.
    for (int fd = 4; fd < 64; ++fd) {
        __wasi_prestat_t prestat;
        __wasi_errno_t err = __wasi_fd_prestat_get((__wasi_fd_t)fd, &prestat);
        if (err == __WASI_ERRNO_SUCCESS && prestat.tag == __WASI_PREOPENTYPE_DIR) {
            return fd;
        }
    }
    return -1;
}

static void test_stdio_renumber(void)
{
    // From wasmtime p1_stdio.rs.
    printf("Test 1: stdio renumber behavior\n");
    int fds[] = {STDIN_FILENO, STDOUT_FILENO, STDERR_FILENO};
    for (size_t i = 0; i < sizeof(fds) / sizeof(fds[0]); ++i) {
        __wasi_fdstat_t stat;
        __wasi_errno_t err = __wasi_fd_fdstat_get((__wasi_fd_t)fds[i], &stat);
        assert(err == __WASI_ERRNO_SUCCESS);

        err = __wasi_fd_renumber((__wasi_fd_t)fds[i], (__wasi_fd_t)(fds[i] + 100));
        assert(err == __WASI_ERRNO_BADF);

        err = __wasi_fd_renumber((__wasi_fd_t)fds[i], (__wasi_fd_t)fds[i]);
        assert(err == __WASI_ERRNO_SUCCESS);
    }
}

static void test_basic_renumber(void)
{
    // From wasmtime p1_renumber.rs.
    printf("Test 2: renumber between two files\n");
    int fd_from = open(FILE1, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd_from >= 0);
    int fd_to = open(FILE2, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd_to >= 0);

    __wasi_fdstat_t stat_from;
    __wasi_errno_t err = __wasi_fd_fdstat_get((__wasi_fd_t)fd_from, &stat_from);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_fd_renumber((__wasi_fd_t)fd_from, (__wasi_fd_t)fd_to);
    assert(err == __WASI_ERRNO_SUCCESS);

    // fd_from should now be closed.
    __wasi_fdstat_t closed_stat;
    err = __wasi_fd_fdstat_get((__wasi_fd_t)fd_from, &closed_stat);
    assert(err == __WASI_ERRNO_BADF);

    __wasi_fdstat_t stat_to;
    err = __wasi_fd_fdstat_get((__wasi_fd_t)fd_to, &stat_to);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat_to.fs_filetype == stat_from.fs_filetype);
    assert(stat_to.fs_flags == stat_from.fs_flags);
    assert(stat_to.fs_rights_base == stat_from.fs_rights_base);
    assert(stat_to.fs_rights_inheriting == stat_from.fs_rights_inheriting);

    close(fd_to);
    assert(unlink(FILE1) == 0);
    assert(unlink(FILE2) == 0);
}

static void test_invalid_targets(void)
{
    // From wasmtime p1_renumber.rs and LTP dup201.c (EBADF cases).
    printf("Test 3: invalid renumber targets\n");
    int fd = open(FILE3, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_fd_renumber((__wasi_fd_t)fd, (__wasi_fd_t)127);
    assert(err == __WASI_ERRNO_BADF);

    err = __wasi_fd_renumber((__wasi_fd_t)fd, (__wasi_fd_t)UINT32_MAX);
    assert(err == __WASI_ERRNO_BADF);

    err = __wasi_fd_renumber((__wasi_fd_t)9999, (__wasi_fd_t)fd);
    assert(err == __WASI_ERRNO_BADF);

    close(fd);
    assert(unlink(FILE3) == 0);
}

static void test_overwrite_preopen(void)
{
    // From wasmtime p1_overwrite_preopen.rs.
    printf("Test 4: renumber over preopen\n");
    int pre_fd = find_preopen_fd();
    assert(pre_fd >= 0);

    int dir_fd = open(".", O_RDONLY);
    assert(dir_fd >= 0);
    assert(dir_fd != pre_fd);

    __wasi_filestat_t old_stat;
    __wasi_errno_t err =
        __wasi_fd_filestat_get((__wasi_fd_t)dir_fd, &old_stat);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_fd_renumber((__wasi_fd_t)dir_fd, (__wasi_fd_t)pre_fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filestat_t new_stat;
    err = __wasi_fd_filestat_get((__wasi_fd_t)pre_fd, &new_stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(new_stat.dev == old_stat.dev);
    assert(new_stat.ino == old_stat.ino);

    __wasi_fdstat_t closed_stat;
    err = __wasi_fd_fdstat_get((__wasi_fd_t)dir_fd, &closed_stat);
    assert(err == __WASI_ERRNO_BADF);
}

int main(void)
{
    test_basic_renumber();
    test_invalid_targets();
    test_overwrite_preopen();
    test_stdio_renumber();
    printf("All tests passed!\n");
    return 0;
}
