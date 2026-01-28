#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
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

static void unlink_if_exists(int dir_fd, const char *path)
{
    __wasi_errno_t err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, path);
    if (err != __WASI_ERRNO_SUCCESS) {
        assert(err == __WASI_ERRNO_NOENT);
    }
}

static void rmdir_if_exists(int dir_fd, const char *path)
{
    __wasi_errno_t err = __wasi_path_remove_directory((__wasi_fd_t)dir_fd, path);
    if (err != __WASI_ERRNO_SUCCESS) {
        assert(err == __WASI_ERRNO_NOENT);
    }
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

static void test_basic_create(int dir_fd)
{
    printf("Test 1: create dir and file inside\n");
    __wasi_errno_t err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pcd_basic");
    assert(err == __WASI_ERRNO_SUCCESS);

    int fd = open("pcd_basic/file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pcd_basic/file") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_remove_directory((__wasi_fd_t)dir_fd, "pcd_basic") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_nested_create(int dir_fd)
{
    printf("Test 2: create nested directory\n");
    remove_path_if_exists(dir_fd, "pcd_nested/child");
    remove_path_if_exists(dir_fd, "pcd_nested");
    __wasi_errno_t err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pcd_nested");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pcd_nested/child");
    assert(err == __WASI_ERRNO_SUCCESS);

    assert(__wasi_path_remove_directory((__wasi_fd_t)dir_fd, "pcd_nested/child") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_remove_directory((__wasi_fd_t)dir_fd, "pcd_nested") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_trailing_slash(int dir_fd)
{
    printf("Test 3: trailing slash\n");
    __wasi_errno_t err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pcd_trailing/");
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_remove_directory((__wasi_fd_t)dir_fd, "pcd_trailing") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_exists_dir(int dir_fd)
{
    printf("Test 4: existing directory\n");
    __wasi_errno_t err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pcd_exist_dir");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pcd_exist_dir");
    assert(err == __WASI_ERRNO_EXIST);

    assert(__wasi_path_remove_directory((__wasi_fd_t)dir_fd, "pcd_exist_dir") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_exists_file(int dir_fd)
{
    printf("Test 5: existing file\n");
    int fd = open("pcd_exist_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);

    __wasi_errno_t err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pcd_exist_file");
    assert(err == __WASI_ERRNO_EXIST);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pcd_exist_file") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_missing_parent(int dir_fd)
{
    printf("Test 6: missing parent\n");
    rmdir_if_exists(dir_fd, "pcd_noent_parent");

    __wasi_errno_t err =
        __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pcd_noent_parent/child");
    assert(err == __WASI_ERRNO_NOENT);
}

static void test_notdir_component(int dir_fd)
{
    printf("Test 7: non-directory path component\n");
    unlink_if_exists(dir_fd, "pcd_notdir_file");
    int fd = open("pcd_notdir_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);

    __wasi_errno_t err =
        __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pcd_notdir_file/child");
    assert(err == __WASI_ERRNO_NOTDIR);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pcd_notdir_file") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_invalid_fd(void)
{
    printf("Test 8: invalid fd\n");
    __wasi_errno_t err = __wasi_path_create_directory((__wasi_fd_t)9999, "pcd_badfd");
    assert(err == __WASI_ERRNO_BADF);
}

static void test_dirfd_is_file(int dir_fd)
{
    printf("Test 9: dirfd is file\n");
    unlink_if_exists(dir_fd, "pcd_dirfd_file");
    int fd = open("pcd_dirfd_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    __wasi_errno_t err = __wasi_path_create_directory((__wasi_fd_t)fd, "child");
    assert(err == __WASI_ERRNO_NOTDIR);

    assert(close(fd) == 0);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pcd_dirfd_file") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_empty_path(int dir_fd)
{
    printf("Test 10: empty path\n");
    __wasi_errno_t err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, "");
    assert(err == __WASI_ERRNO_NOENT);
}

static void test_symlink_loop(int dir_fd)
{
    printf("Test 11: symlink loop\n");
    unlink_if_exists(dir_fd, "pcd_loop");
    __wasi_errno_t err = __wasi_path_symlink("pcd_loop", (__wasi_fd_t)dir_fd, "pcd_loop");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, "pcd_loop/child");
    assert(err == __WASI_ERRNO_LOOP);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pcd_loop") ==
           __WASI_ERRNO_SUCCESS);
}

int main(void)
{
    int dir_fd = find_preopen_fd();
    assert(dir_fd >= 0);

    test_basic_create(dir_fd);
    test_trailing_slash(dir_fd);
    test_exists_dir(dir_fd);
    test_exists_file(dir_fd);
    test_missing_parent(dir_fd);
    test_notdir_component(dir_fd);
    test_invalid_fd();
    test_dirfd_is_file(dir_fd);
    test_symlink_loop(dir_fd);
    test_nested_create(dir_fd);
    test_empty_path(dir_fd);

    printf("All tests passed!\n");
    return 0;
}
