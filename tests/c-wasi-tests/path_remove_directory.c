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

static void test_basic_remove(int dir_fd)
{
    printf("Test 1: remove empty directory\n");
    remove_path_if_exists(dir_fd, "prd_basic");

    __wasi_errno_t err =
        __wasi_path_create_directory((__wasi_fd_t)dir_fd, "prd_basic");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_remove_directory((__wasi_fd_t)dir_fd, "prd_basic");
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filestat_t stat;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "prd_basic", &stat);
    assert(err == __WASI_ERRNO_NOENT);
}

static void test_nonempty_remove(int dir_fd)
{
    printf("Test 2: remove non-empty directory\n");
    remove_path_if_exists(dir_fd, "prd_nonempty/file");
    remove_path_if_exists(dir_fd, "prd_nonempty");

    __wasi_errno_t err =
        __wasi_path_create_directory((__wasi_fd_t)dir_fd, "prd_nonempty");
    assert(err == __WASI_ERRNO_SUCCESS);

    int fd = open("prd_nonempty/file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);

    err = __wasi_path_remove_directory((__wasi_fd_t)dir_fd, "prd_nonempty");
    assert(err == __WASI_ERRNO_NOTEMPTY);

    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "prd_nonempty/file");
    assert(err == __WASI_ERRNO_SUCCESS);
    err = __wasi_path_remove_directory((__wasi_fd_t)dir_fd, "prd_nonempty");
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_notdir_on_file(int dir_fd)
{
    printf("Test 3: remove file with path_remove_directory\n");
    remove_path_if_exists(dir_fd, "prd_file");
    create_file("prd_file");

    __wasi_errno_t err =
        __wasi_path_remove_directory((__wasi_fd_t)dir_fd, "prd_file");
    assert(err == __WASI_ERRNO_NOTDIR);

    err = __wasi_path_remove_directory((__wasi_fd_t)dir_fd, "prd_file/");
    assert(err == __WASI_ERRNO_NOTDIR);

    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "prd_file");
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_missing_and_badfd(int dir_fd)
{
    printf("Test 4: missing path and invalid fd\n");
    __wasi_filestat_t stat;
    (void)stat;

    __wasi_errno_t err =
        __wasi_path_remove_directory((__wasi_fd_t)dir_fd, "prd_missing");
    assert(err == __WASI_ERRNO_NOENT);

    err = __wasi_path_remove_directory((__wasi_fd_t)9999, "prd_missing");
    assert(err == __WASI_ERRNO_BADF);
}

static void test_prefix_notdir(int dir_fd)
{
    printf("Test 5: prefix is not a directory\n");
    remove_path_if_exists(dir_fd, "prd_prefix");
    create_file("prd_prefix");

    __wasi_errno_t err =
        __wasi_path_remove_directory((__wasi_fd_t)dir_fd, "prd_prefix/child");
    assert(err == __WASI_ERRNO_NOTDIR);

    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "prd_prefix");
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_dirfd_notdir(int dir_fd)
{
    printf("Test 6: dirfd is a file\n");
    remove_path_if_exists(dir_fd, "prd_dirfd_file");
    create_file("prd_dirfd_file");

    int fd = open("prd_dirfd_file", O_RDONLY);
    assert(fd >= 0);

    __wasi_errno_t err =
        __wasi_path_remove_directory((__wasi_fd_t)fd, "child");
    assert(err == __WASI_ERRNO_NOTDIR);

    assert(close(fd) == 0);
    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "prd_dirfd_file");
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_dot_path(int dir_fd)
{
    printf("Test 7: dot path\n");
    __wasi_errno_t err =
        __wasi_path_remove_directory((__wasi_fd_t)dir_fd, ".");
    assert(err == __WASI_ERRNO_INVAL);
}

static void test_symlink_loop(int dir_fd)
{
    printf("Test 8: symlink loop\n");
    remove_path_if_exists(dir_fd, "prd_loop/loop");
    remove_path_if_exists(dir_fd, "prd_loop");

    __wasi_errno_t err =
        __wasi_path_create_directory((__wasi_fd_t)dir_fd, "prd_loop");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_symlink("../prd_loop", (__wasi_fd_t)dir_fd, "prd_loop/loop");
    assert(err == __WASI_ERRNO_SUCCESS);

    char path[1024];
    size_t len = 0;
    for (int i = 0; i < 140; ++i) {
        const char *segment = (i == 0) ? "prd_loop/loop" : "/loop";
        size_t seg_len = strlen(segment);
        assert(len + seg_len + 1 < sizeof(path));
        memcpy(path + len, segment, seg_len);
        len += seg_len;
        path[len] = '\0';
    }

    err = __wasi_path_remove_directory((__wasi_fd_t)dir_fd, path);
    assert(err == __WASI_ERRNO_LOOP);

    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "prd_loop/loop");
    assert(err == __WASI_ERRNO_SUCCESS);
    err = __wasi_path_remove_directory((__wasi_fd_t)dir_fd, "prd_loop");
    assert(err == __WASI_ERRNO_SUCCESS);
}

int main(void)
{
    int dir_fd = find_preopen_fd();
    assert(dir_fd >= 0);

    test_basic_remove(dir_fd);
    test_nonempty_remove(dir_fd);
    test_notdir_on_file(dir_fd);
    test_missing_and_badfd(dir_fd);
    test_prefix_notdir(dir_fd);
    test_dirfd_notdir(dir_fd);
    test_dot_path(dir_fd);
    test_symlink_loop(dir_fd);

    printf("All tests passed!\n");
    return 0;
}
