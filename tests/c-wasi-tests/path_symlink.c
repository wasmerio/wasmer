#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
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

static void remove_path_if_exists(int dir_fd, const char *path)
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

static void create_file(const char *name)
{
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);
}

static void test_symlink_to_file(int dir_fd)
{
    printf("Test 1: symlink to file\n");
    remove_path_if_exists(dir_fd, "ps_target_file");
    remove_path_if_exists(dir_fd, "ps_link_file");

    create_file("ps_target_file");

    __wasi_errno_t err =
        __wasi_path_symlink("ps_target_file", (__wasi_fd_t)dir_fd, "ps_link_file");
    assert(err == __WASI_ERRNO_SUCCESS);

    uint8_t buf[32];
    __wasi_size_t used = 0;
    err = __wasi_path_readlink((__wasi_fd_t)dir_fd, "ps_link_file", buf, sizeof(buf), &used);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(used == strlen("ps_target_file"));
    assert(memcmp(buf, "ps_target_file", used) == 0);

    __wasi_filestat_t stat;
    err = __wasi_path_filestat_get(
        (__wasi_fd_t)dir_fd,
        __WASI_LOOKUPFLAGS_SYMLINK_FOLLOW,
        "ps_link_file",
        &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_REGULAR_FILE);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "ps_link_file") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "ps_target_file") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_symlink_to_dir(int dir_fd)
{
    printf("Test 2: symlink to directory\n");
    remove_path_if_exists(dir_fd, "ps_target_dir");
    remove_path_if_exists(dir_fd, "ps_link_dir");

    __wasi_errno_t err =
        __wasi_path_create_directory((__wasi_fd_t)dir_fd, "ps_target_dir");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_symlink("ps_target_dir", (__wasi_fd_t)dir_fd, "ps_link_dir");
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filestat_t stat;
    err = __wasi_path_filestat_get(
        (__wasi_fd_t)dir_fd,
        __WASI_LOOKUPFLAGS_SYMLINK_FOLLOW,
        "ps_link_dir",
        &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_DIRECTORY);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "ps_link_dir") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_remove_directory((__wasi_fd_t)dir_fd, "ps_target_dir") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_dangling_symlink(int dir_fd)
{
    printf("Test 3: dangling symlink\n");
    remove_path_if_exists(dir_fd, "ps_dangling_link");

    __wasi_errno_t err =
        __wasi_path_symlink("ps_dangling_target", (__wasi_fd_t)dir_fd, "ps_dangling_link");
    assert(err == __WASI_ERRNO_SUCCESS);

    uint8_t buf[32];
    __wasi_size_t used = 0;
    err = __wasi_path_readlink((__wasi_fd_t)dir_fd, "ps_dangling_link", buf, sizeof(buf), &used);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(used == strlen("ps_dangling_target"));
    assert(memcmp(buf, "ps_dangling_target", used) == 0);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "ps_dangling_link") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_trailing_slashes(int dir_fd)
{
    printf("Test 4: trailing slash behavior\n");
    remove_path_if_exists(dir_fd, "ps_target");

    __wasi_errno_t err =
        __wasi_path_symlink("source", (__wasi_fd_t)dir_fd, "ps_target/");
    assert(err == __WASI_ERRNO_NOENT);

    err = __wasi_path_symlink("source", (__wasi_fd_t)dir_fd, "ps_target");
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "ps_target") ==
           __WASI_ERRNO_SUCCESS);

    err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, "ps_target");
    assert(err == __WASI_ERRNO_SUCCESS);
    err = __wasi_path_symlink("source", (__wasi_fd_t)dir_fd, "ps_target/");
    assert(err == __WASI_ERRNO_EXIST);
    err = __wasi_path_symlink("source", (__wasi_fd_t)dir_fd, "ps_target");
    assert(err == __WASI_ERRNO_EXIST);
    assert(__wasi_path_remove_directory((__wasi_fd_t)dir_fd, "ps_target") ==
           __WASI_ERRNO_SUCCESS);

    create_file("ps_target");
    err = __wasi_path_symlink("source", (__wasi_fd_t)dir_fd, "ps_target/");
    assert(err == __WASI_ERRNO_NOTDIR);
    err = __wasi_path_symlink("source", (__wasi_fd_t)dir_fd, "ps_target");
    assert(err == __WASI_ERRNO_EXIST);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "ps_target") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_error_cases(int dir_fd)
{
    printf("Test 5: error cases\n");
    remove_path_if_exists(dir_fd, "ps_parent_file");
    create_file("ps_parent_file");

    __wasi_errno_t err =
        __wasi_path_symlink("source", (__wasi_fd_t)dir_fd, "ps_parent_file/child");
    assert(err == __WASI_ERRNO_NOTDIR);

    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "ps_parent_file");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_symlink("source", (__wasi_fd_t)dir_fd, "ps_missing/child");
    assert(err == __WASI_ERRNO_NOENT);

    err = __wasi_path_symlink("source", (__wasi_fd_t)9999, "ps_badfd");
    assert(err == __WASI_ERRNO_BADF);

    err = __wasi_path_symlink("/", (__wasi_fd_t)dir_fd, "ps_abs_target");
    assert(err != __WASI_ERRNO_SUCCESS);
}

int main(void)
{
    int dir_fd = find_preopen_fd();
    assert(dir_fd >= 0);

    test_symlink_to_file(dir_fd);
    test_symlink_to_dir(dir_fd);
    test_dangling_symlink(dir_fd);
    test_trailing_slashes(dir_fd);
    test_error_cases(dir_fd);

    printf("All tests passed!\n");
    return 0;
}
