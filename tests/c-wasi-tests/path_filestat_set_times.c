#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <unistd.h>

#include <wasi/api_wasi.h>

static const uint64_t kTimeTolerance = 2000000000ULL;

static int find_preopen_fd(void)
{
    for (int fd = 4; fd < 64; ++fd) {
        __wasi_prestat_t prestat;
        __wasi_errno_t err = __wasi_fd_prestat_get((__wasi_fd_t)fd, &prestat);
        if (err == __WASI_ERRNO_SUCCESS && prestat.tag == __WASI_PREOPENTYPE_DIR) {
            return fd;
        }
    }
    return -1;
}

static void remove_if_exists(int dir_fd, const char *path)
{
    __wasi_filestat_t stat;
    __wasi_errno_t err =
        __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, path, &stat);
    if (err == __WASI_ERRNO_SUCCESS) {
        if (stat.filetype == __WASI_FILETYPE_DIRECTORY) {
            err = __wasi_path_remove_directory((__wasi_fd_t)dir_fd, path);
        } else {
            err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, path);
        }
        assert(err == __WASI_ERRNO_SUCCESS);
    } else {
        assert(err == __WASI_ERRNO_NOENT);
    }
}

static uint64_t now_nanos(void)
{
    __wasi_timestamp_t ts = 0;
    __wasi_errno_t err = __wasi_clock_time_get(__WASI_CLOCKID_REALTIME, 1, &ts);
    assert(err == __WASI_ERRNO_SUCCESS);
    return ts;
}

static void assert_time_close(uint64_t actual, uint64_t expected, uint64_t tolerance)
{
    if (actual >= expected) {
        assert(actual - expected <= tolerance);
    } else {
        assert(expected - actual <= tolerance);
    }
}

static void assert_time_between(uint64_t actual, uint64_t before, uint64_t after, uint64_t tolerance)
{
    uint64_t lower = (before > tolerance) ? (before - tolerance) : 0;
    uint64_t upper = after + tolerance;
    assert(actual >= lower);
    assert(actual <= upper);
}

static void create_empty_file(const char *name)
{
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);
}

static void test_set_mtim_and_invalid_flags(int dir_fd)
{
    // From wasmtime p1_path_filestat.rs: set mtim and invalid flag combos.
    printf("Test 1: set mtim + invalid flag combinations\n");
    remove_if_exists(dir_fd, "pfs_times_file");
    create_empty_file("pfs_times_file");

    __wasi_filestat_t stat;
    __wasi_errno_t err =
        __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_times_file", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint64_t old_atim = stat.atim;
    uint64_t old_mtim = stat.mtim;
    uint64_t delta = 2000000000ULL;
    uint64_t new_mtim = (old_mtim > delta) ? (old_mtim - delta) : (old_mtim + delta);

    err = __wasi_path_filestat_set_times(
        (__wasi_fd_t)dir_fd,
        0,
        "pfs_times_file",
        0,
        new_mtim,
        __WASI_FSTFLAGS_MTIM);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_times_file", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_time_close(stat.mtim, new_mtim, kTimeTolerance);
    assert_time_close(stat.atim, old_atim, kTimeTolerance);

    err = __wasi_path_filestat_set_times(
        (__wasi_fd_t)dir_fd,
        0,
        "pfs_times_file",
        0,
        0,
        (__wasi_fstflags_t)(__WASI_FSTFLAGS_ATIM | __WASI_FSTFLAGS_ATIM_NOW));
    assert(err == __WASI_ERRNO_INVAL);

    err = __wasi_path_filestat_set_times(
        (__wasi_fd_t)dir_fd,
        0,
        "pfs_times_file",
        0,
        0,
        (__wasi_fstflags_t)(__WASI_FSTFLAGS_MTIM | __WASI_FSTFLAGS_MTIM_NOW));
    assert(err == __WASI_ERRNO_INVAL);

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_times_file", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_time_close(stat.mtim, new_mtim, kTimeTolerance);

    remove_if_exists(dir_fd, "pfs_times_file");
}

static void test_atim_now_only(int dir_fd)
{
    // From gVisor utimes.cc: ATIM_NOW updates atime only.
    printf("Test 2: ATIM_NOW only\n");
    remove_if_exists(dir_fd, "pfs_atim_now");
    create_empty_file("pfs_atim_now");

    __wasi_filestat_t stat;
    __wasi_errno_t err =
        __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_atim_now", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    uint64_t old_mtim = stat.mtim;

    uint64_t before = now_nanos();
    err = __wasi_path_filestat_set_times(
        (__wasi_fd_t)dir_fd,
        0,
        "pfs_atim_now",
        0,
        0,
        __WASI_FSTFLAGS_ATIM_NOW);
    assert(err == __WASI_ERRNO_SUCCESS);
    uint64_t after = now_nanos();

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_atim_now", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_time_between(stat.atim, before, after, kTimeTolerance);
    assert_time_close(stat.mtim, old_mtim, kTimeTolerance);

    remove_if_exists(dir_fd, "pfs_atim_now");
}

static void test_mtim_now_only(int dir_fd)
{
    // From gVisor utimes.cc: MTIM_NOW updates mtime only.
    printf("Test 3: MTIM_NOW only\n");
    remove_if_exists(dir_fd, "pfs_mtim_now");
    create_empty_file("pfs_mtim_now");

    __wasi_filestat_t stat;
    __wasi_errno_t err =
        __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_mtim_now", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    uint64_t old_atim = stat.atim;

    uint64_t before = now_nanos();
    err = __wasi_path_filestat_set_times(
        (__wasi_fd_t)dir_fd,
        0,
        "pfs_mtim_now",
        0,
        0,
        __WASI_FSTFLAGS_MTIM_NOW);
    assert(err == __WASI_ERRNO_SUCCESS);
    uint64_t after = now_nanos();

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_mtim_now", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_time_between(stat.mtim, before, after, kTimeTolerance);
    assert_time_close(stat.atim, old_atim, kTimeTolerance);

    remove_if_exists(dir_fd, "pfs_mtim_now");
}

static void test_both_now(int dir_fd)
{
    // From gVisor utimes.cc: times=NULL equivalent (both now).
    printf("Test 4: ATIM_NOW + MTIM_NOW\n");
    remove_if_exists(dir_fd, "pfs_both_now");
    create_empty_file("pfs_both_now");

    uint64_t before = now_nanos();
    __wasi_errno_t err = __wasi_path_filestat_set_times(
        (__wasi_fd_t)dir_fd,
        0,
        "pfs_both_now",
        0,
        0,
        (__wasi_fstflags_t)(__WASI_FSTFLAGS_ATIM_NOW | __WASI_FSTFLAGS_MTIM_NOW));
    assert(err == __WASI_ERRNO_SUCCESS);
    uint64_t after = now_nanos();

    __wasi_filestat_t stat;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_both_now", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_time_between(stat.atim, before, after, kTimeTolerance);
    assert_time_between(stat.mtim, before, after, kTimeTolerance);

    remove_if_exists(dir_fd, "pfs_both_now");
}

static void test_symlink_follow(int dir_fd)
{
    // From wasmtime p1_symlink_filestat.rs: follow vs nofollow.
    printf("Test 5: symlink follow vs nofollow\n");
    remove_if_exists(dir_fd, "pfs_target");
    remove_if_exists(dir_fd, "pfs_symlink");
    create_empty_file("pfs_target");
    __wasi_errno_t err =
        __wasi_path_symlink("pfs_target", (__wasi_fd_t)dir_fd, "pfs_symlink");
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filestat_t file_stat;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_target", &file_stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    uint64_t old_file_mtim = file_stat.mtim;

    __wasi_filestat_t sym_stat;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_symlink", &sym_stat);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint64_t delta = 2000000000ULL;
    uint64_t sym_new_mtim =
        (sym_stat.mtim > delta) ? (sym_stat.mtim - delta) : (sym_stat.mtim + delta);

    err = __wasi_path_filestat_set_times(
        (__wasi_fd_t)dir_fd,
        0,
        "pfs_symlink",
        0,
        sym_new_mtim,
        __WASI_FSTFLAGS_MTIM);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_symlink", &sym_stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_time_close(sym_stat.mtim, sym_new_mtim, kTimeTolerance);

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_target", &file_stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_time_close(file_stat.mtim, old_file_mtim, kTimeTolerance);

    uint64_t file_new_mtim =
        (file_stat.mtim > delta) ? (file_stat.mtim - delta) : (file_stat.mtim + delta);
    err = __wasi_path_filestat_set_times(
        (__wasi_fd_t)dir_fd,
        __WASI_LOOKUPFLAGS_SYMLINK_FOLLOW,
        "pfs_symlink",
        0,
        file_new_mtim,
        __WASI_FSTFLAGS_MTIM);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_target", &file_stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_time_close(file_stat.mtim, file_new_mtim, kTimeTolerance);

    remove_if_exists(dir_fd, "pfs_symlink");
    remove_if_exists(dir_fd, "pfs_target");
}

static void test_directory(int dir_fd)
{
    // From gVisor utimes.cc: directory timestamps update.
    printf("Test 6: set times on directory\n");
    remove_if_exists(dir_fd, "pfs_dir");
    __wasi_errno_t err =
        __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pfs_dir");
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filestat_t stat;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_dir", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint64_t delta = 2000000000ULL;
    uint64_t new_mtim = (stat.mtim > delta) ? (stat.mtim - delta) : (stat.mtim + delta);
    err = __wasi_path_filestat_set_times(
        (__wasi_fd_t)dir_fd,
        0,
        "pfs_dir",
        0,
        new_mtim,
        __WASI_FSTFLAGS_MTIM);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfs_dir", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_time_close(stat.mtim, new_mtim, kTimeTolerance);

    remove_if_exists(dir_fd, "pfs_dir");
}

static void test_missing_path(int dir_fd)
{
    // From gVisor utimes.cc: missing path -> NOENT.
    printf("Test 7: missing path\n");
    remove_if_exists(dir_fd, "pfs_missing");
    __wasi_errno_t err = __wasi_path_filestat_set_times(
        (__wasi_fd_t)dir_fd,
        0,
        "pfs_missing",
        0,
        0,
        __WASI_FSTFLAGS_MTIM_NOW);
    assert(err == __WASI_ERRNO_NOENT);
}

int main(void)
{
    int dir_fd = find_preopen_fd();
    assert(dir_fd >= 0);

    test_set_mtim_and_invalid_flags(dir_fd);
    test_symlink_follow(dir_fd);
    test_directory(dir_fd);
    test_missing_path(dir_fd);
    test_atim_now_only(dir_fd);
    test_mtim_now_only(dir_fd);
    test_both_now(dir_fd);

    printf("All tests passed!\n");
    return 0;
}
