#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

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

static void assert_time_close(uint64_t actual, uint64_t expected, uint64_t tolerance)
{
    if (actual >= expected) {
        assert(actual - expected <= tolerance);
    } else {
        assert(expected - actual <= tolerance);
    }
}

static void test_basic_file_stats(int dir_fd)
{
    printf("Test 1: basic file stats\n");
    remove_if_exists(dir_fd, "pfg_file");
    int fd = open("pfg_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    char data[200];
    memset(data, 'a', sizeof(data));
    assert(write(fd, data, sizeof(data)) == (ssize_t)sizeof(data));
    assert(close(fd) == 0);

    __wasi_filestat_t stat;
    __wasi_errno_t err =
        __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfg_file", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_REGULAR_FILE);
    assert(stat.size == sizeof(data));

    remove_if_exists(dir_fd, "pfg_file");
}

static void test_directory_stats(int dir_fd)
{
    printf("Test 2: directory stats\n");
    remove_if_exists(dir_fd, "pfg_dir");
    __wasi_errno_t err =
        __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pfg_dir");
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filestat_t stat;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfg_dir", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_DIRECTORY);

    remove_if_exists(dir_fd, "pfg_dir");
}

static void test_symlink_stats(int dir_fd)
{
    printf("Test 3: symlink stats (follow vs nofollow)\n");
    remove_if_exists(dir_fd, "pfg_target_file");
    remove_if_exists(dir_fd, "pfg_target_dir");
    remove_if_exists(dir_fd, "pfg_link_file");
    remove_if_exists(dir_fd, "pfg_link_dir");

    int fd = open("pfg_target_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);
    __wasi_errno_t err =
        __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pfg_target_dir");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_symlink("pfg_target_file", (__wasi_fd_t)dir_fd, "pfg_link_file");
    assert(err == __WASI_ERRNO_SUCCESS);
    err = __wasi_path_symlink("pfg_target_dir", (__wasi_fd_t)dir_fd, "pfg_link_dir");
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filestat_t stat;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfg_link_file", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_SYMBOLIC_LINK);

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfg_link_dir", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_SYMBOLIC_LINK);

    err = __wasi_path_filestat_get(
        (__wasi_fd_t)dir_fd,
        __WASI_LOOKUPFLAGS_SYMLINK_FOLLOW,
        "pfg_link_file",
        &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_REGULAR_FILE);

    err = __wasi_path_filestat_get(
        (__wasi_fd_t)dir_fd,
        __WASI_LOOKUPFLAGS_SYMLINK_FOLLOW,
        "pfg_link_dir",
        &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_DIRECTORY);

    remove_if_exists(dir_fd, "pfg_link_file");
    remove_if_exists(dir_fd, "pfg_link_dir");
    remove_if_exists(dir_fd, "pfg_target_file");
    remove_if_exists(dir_fd, "pfg_target_dir");
}

static void test_set_times_reflected(int dir_fd)
{
    printf("Test 4: mtim updates reflected in path_filestat_get\n");
    remove_if_exists(dir_fd, "pfg_times_file");
    int fd = open("pfg_times_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);

    __wasi_filestat_t stat;
    __wasi_errno_t err =
        __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfg_times_file", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);

    uint64_t delta = 2000000000ULL;
    uint64_t new_mtim = (stat.mtim > delta) ? (stat.mtim - delta) : (stat.mtim + delta);
    err = __wasi_path_filestat_set_times(
        (__wasi_fd_t)dir_fd,
        0,
        "pfg_times_file",
        0,
        new_mtim,
        __WASI_FSTFLAGS_MTIM);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfg_times_file", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_time_close(stat.mtim, new_mtim, 1000000000ULL);

    remove_if_exists(dir_fd, "pfg_times_file");
}

static void test_errors(int dir_fd)
{
    printf("Test 5: error cases\n");
    __wasi_filestat_t stat;
    __wasi_errno_t err =
        __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pfg_missing", &stat);
    assert(err == __WASI_ERRNO_NOENT);

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 9999, "pfg_missing", &stat);
    assert(err == __WASI_ERRNO_INVAL);

    err = __wasi_path_filestat_get((__wasi_fd_t)9999, 0, "pfg_missing", &stat);
    assert(err == __WASI_ERRNO_BADF);

    remove_if_exists(dir_fd, "pfg_dirfd_file");
    int fd = open("pfg_dirfd_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    err = __wasi_path_filestat_get((__wasi_fd_t)fd, 0, "child", &stat);
    assert(err == __WASI_ERRNO_NOTDIR);
    assert(close(fd) == 0);
    remove_if_exists(dir_fd, "pfg_dirfd_file");
}

int main(void)
{
    int dir_fd = find_preopen_fd();
    assert(dir_fd >= 0);

    test_basic_file_stats(dir_fd);
    test_directory_stats(dir_fd);
    test_symlink_stats(dir_fd);
    test_set_times_reflected(dir_fd);
    test_errors(dir_fd);

    printf("All tests passed!\n");
    return 0;
}
