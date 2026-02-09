#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

#ifndef __WASI_ERRNO_MEMVIOLATION
#define __WASI_ERRNO_MEMVIOLATION (UINT16_C(78))
#endif

__attribute__((__import_module__("wasi_snapshot_preview1"),
               __import_name__("path_unlink_file")))
extern __wasi_errno_t wasi_path_unlink_file_raw(__wasi_fd_t fd,
                                                const char *path,
                                                __wasi_size_t path_len);

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

static void test_basic_unlink(int dir_fd)
{
    printf("Test 1: unlink regular file\n");
    remove_path_if_exists(dir_fd, "puf_basic");
    create_file("puf_basic");

    __wasi_errno_t err =
        __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_basic");
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filestat_t stat;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "puf_basic", &stat);
    assert(err == __WASI_ERRNO_NOENT);
}

static void test_open_file_unlink(int dir_fd)
{
    printf("Test 2: unlink open file\n");
    remove_path_if_exists(dir_fd, "puf_open");
    create_file("puf_open");

    int fd = open("puf_open", O_RDWR);
    assert(fd >= 0);

    __wasi_errno_t err =
        __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_open");
    assert(err == __WASI_ERRNO_SUCCESS);

    const char data[] = "hi";
    assert(write(fd, data, sizeof(data)) == (ssize_t)sizeof(data));
    assert(close(fd) == 0);

    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_open");
    assert(err == __WASI_ERRNO_NOENT);
}

static void test_unlink_directory(int dir_fd)
{
    printf("Test 3: unlink directory (ISDIR)\n");
    remove_path_if_exists(dir_fd, "puf_dir");

    __wasi_errno_t err =
        __wasi_path_create_directory((__wasi_fd_t)dir_fd, "puf_dir");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_dir");
    assert(err == __WASI_ERRNO_ISDIR);

    __wasi_filestat_t stat;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "puf_dir", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_DIRECTORY);

    err = __wasi_path_remove_directory((__wasi_fd_t)dir_fd, "puf_dir");
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_trailing_slashes(int dir_fd)
{
    printf("Test 4: trailing slash behavior\n");
    remove_path_if_exists(dir_fd, "puf_trailing_file");
    remove_path_if_exists(dir_fd, "puf_trailing_dir");

    create_file("puf_trailing_file");
    __wasi_errno_t err =
        __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_trailing_file/");
    assert(err == __WASI_ERRNO_NOTDIR);
    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_trailing_file");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, "puf_trailing_dir");
    assert(err == __WASI_ERRNO_SUCCESS);
    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_trailing_dir/");
    assert(err == __WASI_ERRNO_ISDIR);
    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_trailing_dir");
    assert(err == __WASI_ERRNO_ISDIR);
    err = __wasi_path_remove_directory((__wasi_fd_t)dir_fd, "puf_trailing_dir");
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_error_cases(int dir_fd)
{
    printf("Test 5: error cases\n");
    __wasi_errno_t err =
        __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_missing");
    assert(err == __WASI_ERRNO_NOENT);

    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "");
    assert(err == __WASI_ERRNO_NOENT);

    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_missing/child");
    assert(err == __WASI_ERRNO_NOENT);

    remove_path_if_exists(dir_fd, "puf_notdir");
    create_file("puf_notdir");
    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_notdir/child");
    assert(err == __WASI_ERRNO_NOTDIR);
    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_notdir/.");
    assert(err == __WASI_ERRNO_NOTDIR);
    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_notdir/..");
    assert(err == __WASI_ERRNO_NOTDIR);
    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, "puf_notdir");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_unlink_file((__wasi_fd_t)9999, "puf_badfd");
    assert(err == __WASI_ERRNO_BADF);

    const char *bad = (const char *)(uintptr_t)0xffffffffu;
    err = wasi_path_unlink_file_raw((__wasi_fd_t)dir_fd, bad, 1);
    assert(err == __WASI_ERRNO_MEMVIOLATION);

    char longname[300];
    memset(longname, 'a', sizeof(longname) - 1);
    longname[sizeof(longname) - 1] = '\0';
    err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, longname);
    assert(err == __WASI_ERRNO_NAMETOOLONG);
}

int main(void)
{
    int dir_fd = find_preopen_fd();
    assert(dir_fd >= 0);

    test_basic_unlink(dir_fd);
    test_open_file_unlink(dir_fd);
    test_unlink_directory(dir_fd);
    test_trailing_slashes(dir_fd);
    test_error_cases(dir_fd);

    printf("All tests passed!\n");
    return 0;
}
