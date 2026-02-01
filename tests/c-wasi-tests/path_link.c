#include <assert.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

static int find_preopen_fd(void)
{
    int fallback_fd = -1;
    for (int fd = 4; fd < 64; ++fd) {
        __wasi_prestat_t prestat;
        __wasi_errno_t err = __wasi_fd_prestat_get((__wasi_fd_t)fd, &prestat);
        if (err == __WASI_ERRNO_SUCCESS && prestat.tag == __WASI_PREOPENTYPE_DIR) {
            __wasi_size_t len = prestat.u.dir.pr_name_len;
            char *name = (char *)malloc(len + 1);
            assert(name != NULL);
            err = __wasi_fd_prestat_dir_name((__wasi_fd_t)fd, (uint8_t *)name, len);
            assert(err == __WASI_ERRNO_SUCCESS);
            name[len] = '\0';
            if (strcmp(name, ".") == 0) {
                free(name);
                return fd;
            }
            if (fallback_fd == -1 && strcmp(name, "/dev") != 0) {
                fallback_fd = fd;
            }
            free(name);
        }
    }
    return fallback_fd;
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

static void create_file_with_data(const char *name, const char *data)
{
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    size_t len = strlen(data);
    assert(write(fd, data, len) == (ssize_t)len);
    assert(close(fd) == 0);
}

static void create_file(const char *name)
{
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);
}

static __wasi_fd_t open_dir_fd(int dir_fd, const char *path)
{
    __wasi_fd_t out_fd = 0;
    __wasi_rights_t rights =
        __WASI_RIGHTS_PATH_LINK_SOURCE | __WASI_RIGHTS_PATH_LINK_TARGET |
        __WASI_RIGHTS_PATH_OPEN | __WASI_RIGHTS_PATH_FILESTAT_GET |
        __WASI_RIGHTS_PATH_READLINK | __WASI_RIGHTS_PATH_CREATE_DIRECTORY |
        __WASI_RIGHTS_PATH_CREATE_FILE | __WASI_RIGHTS_PATH_UNLINK_FILE |
        __WASI_RIGHTS_PATH_REMOVE_DIRECTORY;
    __wasi_errno_t err = __wasi_path_open(
        (__wasi_fd_t)dir_fd, 0, path, __WASI_OFLAGS_DIRECTORY, rights, rights, 0, &out_fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    return out_fd;
}

static int open_file_at(int dir_fd, const char *path)
{
    __wasi_fd_t out_fd = 0;
    __wasi_rights_t rights =
        __WASI_RIGHTS_FD_READ | __WASI_RIGHTS_FD_SEEK | __WASI_RIGHTS_FD_TELL |
        __WASI_RIGHTS_FD_FILESTAT_GET;
    __wasi_errno_t err = __wasi_path_open(
        (__wasi_fd_t)dir_fd, 0, path, 0, rights, 0, 0, &out_fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    return (int)out_fd;
}

static void assert_readlink_eq(int dir_fd, const char *path, const char *expected)
{
    uint8_t buf[64];
    __wasi_size_t used = 0;
    __wasi_errno_t err = __wasi_path_readlink(
        (__wasi_fd_t)dir_fd, path, buf, sizeof(buf), &used);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(used == strlen(expected));
    assert(memcmp(buf, expected, used) == 0);
}

static void test_basic_link(int dir_fd)
{
    printf("Test 1: basic hard link\n");
    remove_path_if_exists(dir_fd, "pl_file");
    remove_path_if_exists(dir_fd, "pl_link");

    create_file_with_data("pl_file", "abc");

    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_file", (__wasi_fd_t)dir_fd, "pl_link");
    assert(err == __WASI_ERRNO_SUCCESS);

    int fd_file = open_file_at(dir_fd, "pl_file");
    int fd_link = open_file_at(dir_fd, "pl_link");

    char buf[4] = {0};
    assert(read(fd_link, buf, 3) == 3);
    assert(strcmp(buf, "abc") == 0);

    __wasi_filestat_t st_file;
    __wasi_filestat_t st_link;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pl_file", &st_file);
    assert(err == __WASI_ERRNO_SUCCESS);
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pl_link", &st_link);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(st_file.dev == st_link.dev);
    assert(st_file.ino == st_link.ino);
    assert(st_file.size == st_link.size);
    assert(st_file.nlink == st_link.nlink);

    assert(__wasi_fd_close((__wasi_fd_t)fd_link) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close((__wasi_fd_t)fd_file) == __WASI_ERRNO_SUCCESS);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_link") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_file") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_link_into_subdir(int dir_fd)
{
    printf("Test 2: link into subdirectory\n");
    remove_path_if_exists(dir_fd, "pl_subdir");
    remove_path_if_exists(dir_fd, "pl_file2");

    assert(__wasi_path_create_directory((__wasi_fd_t)dir_fd, "pl_subdir") ==
           __WASI_ERRNO_SUCCESS);
    create_file_with_data("pl_file2", "xyz");

    __wasi_fd_t subdir_fd = open_dir_fd(dir_fd, "pl_subdir");
    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_file2", subdir_fd, "pl_link");
    assert(err == __WASI_ERRNO_SUCCESS);

    int fd_link = open_file_at(dir_fd, "pl_subdir/pl_link");
    char buf[4] = {0};
    assert(read(fd_link, buf, 3) == 3);
    assert(strcmp(buf, "xyz") == 0);
    assert(__wasi_fd_close((__wasi_fd_t)fd_link) == __WASI_ERRNO_SUCCESS);

    assert(__wasi_fd_close(subdir_fd) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_subdir/pl_link") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_file2") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_remove_directory((__wasi_fd_t)dir_fd, "pl_subdir") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_existing_target(int dir_fd)
{
    printf("Test 3: target exists\n");
    remove_path_if_exists(dir_fd, "pl_file3");
    remove_path_if_exists(dir_fd, "pl_link");

    create_file("pl_file3");
    create_file("pl_link");
    __wasi_filestat_t st_link;
    __wasi_errno_t st_err =
        __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pl_link", &st_link);
    assert(st_err == __WASI_ERRNO_SUCCESS);

    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_file3", (__wasi_fd_t)dir_fd, "pl_link");
    if (err != __WASI_ERRNO_EXIST) {
        fprintf(stderr, "Expected __WASI_ERRNO_EXIST, got %u\n", err);
        fflush(stderr);
    }
    assert(err == __WASI_ERRNO_EXIST);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_link") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_file3") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_link_to_self(int dir_fd)
{
    printf("Test 4: link to self\n");
    remove_path_if_exists(dir_fd, "pl_file4");
    create_file("pl_file4");

    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_file4", (__wasi_fd_t)dir_fd, "pl_file4");
    assert(err == __WASI_ERRNO_EXIST);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_file4") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_target_is_dir(int dir_fd)
{
    printf("Test 5: target is directory\n");
    remove_path_if_exists(dir_fd, "pl_file5");
    remove_path_if_exists(dir_fd, "pl_dir");

    create_file("pl_file5");
    assert(__wasi_path_create_directory((__wasi_fd_t)dir_fd, "pl_dir") ==
           __WASI_ERRNO_SUCCESS);

    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_file5", (__wasi_fd_t)dir_fd, "pl_dir");
    assert(err == __WASI_ERRNO_EXIST);

    assert(__wasi_path_remove_directory((__wasi_fd_t)dir_fd, "pl_dir") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_file5") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_source_is_dir(int dir_fd)
{
    printf("Test 6: source is directory\n");
    remove_path_if_exists(dir_fd, "pl_srcdir");
    remove_path_if_exists(dir_fd, "pl_link");

    assert(__wasi_path_create_directory((__wasi_fd_t)dir_fd, "pl_srcdir") ==
           __WASI_ERRNO_SUCCESS);

    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_srcdir", (__wasi_fd_t)dir_fd, "pl_link");
    if (err != __WASI_ERRNO_PERM && err != __WASI_ERRNO_ACCES) {
        fprintf(stderr, "Expected __WASI_ERRNO_PERM or __WASI_ERRNO_ACCES, got %u\n", err);
        fflush(stderr);
    }
    assert(err == __WASI_ERRNO_PERM || err == __WASI_ERRNO_ACCES);

    assert(__wasi_path_remove_directory((__wasi_fd_t)dir_fd, "pl_srcdir") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_missing_source(int dir_fd)
{
    printf("Test 7: missing source\n");
    remove_path_if_exists(dir_fd, "pl_missing");
    remove_path_if_exists(dir_fd, "pl_link");

    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_missing", (__wasi_fd_t)dir_fd, "pl_link");
    assert(err == __WASI_ERRNO_NOENT);
}

static void test_missing_parent(int dir_fd)
{
    printf("Test 8: missing parent directory\n");
    remove_path_if_exists(dir_fd, "pl_file6");
    create_file("pl_file6");

    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_file6", (__wasi_fd_t)dir_fd, "no_dir/pl_link");
    assert(err == __WASI_ERRNO_NOENT);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_file6") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_fd_errors(int dir_fd)
{
    printf("Test 9: bad fd and notdir cases\n");
    remove_path_if_exists(dir_fd, "pl_file7");
    create_file("pl_file7");

    int file_fd = open("pl_file7", O_RDONLY);
    assert(file_fd >= 0);

    __wasi_errno_t err = __wasi_path_link(
        9999, 0, "pl_file7", (__wasi_fd_t)dir_fd, "pl_link");
    assert(err == __WASI_ERRNO_BADF);

    err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_file7", 9999, "pl_link");
    assert(err == __WASI_ERRNO_BADF);

    err = __wasi_path_link(
        (__wasi_fd_t)file_fd, 0, "pl_file7", (__wasi_fd_t)dir_fd, "pl_link");
    assert(err == __WASI_ERRNO_NOTDIR);

    err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_file7", (__wasi_fd_t)file_fd, "pl_link");
    assert(err == __WASI_ERRNO_NOTDIR);

    assert(close(file_fd) == 0);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_file7") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_trailing_slash(int dir_fd)
{
    printf("Test 10: trailing slash on target\n");
    remove_path_if_exists(dir_fd, "pl_file8");
    remove_path_if_exists(dir_fd, "pl_link");

    create_file("pl_file8");

    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_file8", (__wasi_fd_t)dir_fd, "pl_link/");
    if (err != __WASI_ERRNO_NOENT) {
        fprintf(stderr, "Expected __WASI_ERRNO_NOENT, got %u\n", err);
        fflush(stderr);
    }
    assert(err == __WASI_ERRNO_NOENT);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_file8") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_empty_oldpath(int dir_fd)
{
    printf("Test 11: empty source path\n");
    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "", (__wasi_fd_t)dir_fd, "pl_link");
    if (err != __WASI_ERRNO_NOENT) {
        fprintf(stderr, "Expected __WASI_ERRNO_NOENT, got %u\n", err);
        fflush(stderr);
    }
    assert(err == __WASI_ERRNO_NOENT);
}

static void test_symlink_no_follow(int dir_fd)
{
    printf("Test 12: link to symlink without follow\n");
    remove_path_if_exists(dir_fd, "pl_symlink");
    remove_path_if_exists(dir_fd, "pl_link");

    __wasi_errno_t err = __wasi_path_symlink("pl_target", (__wasi_fd_t)dir_fd, "pl_symlink");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_symlink", (__wasi_fd_t)dir_fd, "pl_link");
    assert(err == __WASI_ERRNO_SUCCESS);
    assert_readlink_eq(dir_fd, "pl_link", "pl_target");

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_link") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_symlink") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_symlink_follow_invalid(int dir_fd)
{
    printf("Test 13: symlink follow flag\n");
    remove_path_if_exists(dir_fd, "pl_file9");
    remove_path_if_exists(dir_fd, "pl_symlink");
    remove_path_if_exists(dir_fd, "pl_link");

    create_file("pl_file9");
    __wasi_errno_t err = __wasi_path_symlink("pl_file9", (__wasi_fd_t)dir_fd, "pl_symlink");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, __WASI_LOOKUPFLAGS_SYMLINK_FOLLOW, "pl_symlink",
        (__wasi_fd_t)dir_fd, "pl_link");
    if (err != __WASI_ERRNO_INVAL) {
        fprintf(stderr, "Expected __WASI_ERRNO_INVAL, got %u\n", err);
        fflush(stderr);
    }
    assert(err == __WASI_ERRNO_INVAL);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_symlink") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_file9") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_name_too_long(int dir_fd)
{
    printf("Test 14: name too long\n");
    remove_path_if_exists(dir_fd, "pl_file10");
    create_file("pl_file10");

    char longname[300];
    memset(longname, 'a', sizeof(longname) - 1);
    longname[sizeof(longname) - 1] = '\0';

    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_file10", (__wasi_fd_t)dir_fd, longname);
    if (err != __WASI_ERRNO_NAMETOOLONG) {
        fprintf(stderr, "Expected __WASI_ERRNO_NAMETOOLONG, got %u\n", err);
        fflush(stderr);
    }
    assert(err == __WASI_ERRNO_NAMETOOLONG);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_file10") ==
           __WASI_ERRNO_SUCCESS);
}

static void test_link_count(int dir_fd)
{
    printf("Test 15: link count increments\n");
    remove_path_if_exists(dir_fd, "pl_nlink_file");
    remove_path_if_exists(dir_fd, "pl_nlink_link");

    create_file("pl_nlink_file");

    __wasi_errno_t err = __wasi_path_link(
        (__wasi_fd_t)dir_fd, 0, "pl_nlink_file", (__wasi_fd_t)dir_fd, "pl_nlink_link");
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filestat_t st_file;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pl_nlink_file", &st_file);
    assert(err == __WASI_ERRNO_SUCCESS);
    if (st_file.nlink < 2) {
        fprintf(stderr, "Expected nlink >= 2, got %" PRIu64 "\n", st_file.nlink);
        fflush(stderr);
    }
    assert(st_file.nlink >= 2);

    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_nlink_link") ==
           __WASI_ERRNO_SUCCESS);
    assert(__wasi_path_unlink_file((__wasi_fd_t)dir_fd, "pl_nlink_file") ==
           __WASI_ERRNO_SUCCESS);
}

int main(void)
{
    int dir_fd = find_preopen_fd();
    assert(dir_fd >= 0);

    test_basic_link(dir_fd);
    test_link_into_subdir(dir_fd);
    test_link_to_self(dir_fd);
    test_target_is_dir(dir_fd);
    test_existing_target(dir_fd);
    test_missing_source(dir_fd);
    test_missing_parent(dir_fd);
    test_fd_errors(dir_fd);
    test_symlink_no_follow(dir_fd);
    test_trailing_slash(dir_fd);
    test_symlink_follow_invalid(dir_fd);
    test_name_too_long(dir_fd);
    test_source_is_dir(dir_fd);
    test_empty_oldpath(dir_fd);
    test_link_count(dir_fd);

    printf("✓ path_link tests completed\n");
    return 0;
}
