#include <assert.h>
#include <fcntl.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

__wasi_errno_t __wasi_path_rename_raw(
    __wasi_fd_t old_fd,
    const char *old_path,
    __wasi_size_t old_path_len,
    __wasi_fd_t new_fd,
    const char *new_path,
    __wasi_size_t new_path_len
) __attribute__((
    __import_module__("wasi_snapshot_preview1"),
    __import_name__("path_rename")
));

#ifndef NAME_MAX
#define NAME_MAX 255
#endif
#ifndef PATH_MAX
#define PATH_MAX 4096
#endif

static void expect_errno(__wasi_errno_t got, __wasi_errno_t expected, const char *msg)
{
    if (got != expected) {
        fprintf(stderr, "FAIL: %s (got %u expected %u)\n", msg, (unsigned)got, (unsigned)expected);
        assert(got == expected);
    }
}

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

static __wasi_fd_t open_dir_fd(int dir_fd, const char *path)
{
    __wasi_fd_t out_fd = 0;
    __wasi_rights_t base_rights =
        __WASI_RIGHTS_PATH_OPEN |
        __WASI_RIGHTS_PATH_RENAME_SOURCE |
        __WASI_RIGHTS_PATH_RENAME_TARGET |
        __WASI_RIGHTS_PATH_FILESTAT_GET |
        __WASI_RIGHTS_FD_READ |
        __WASI_RIGHTS_PATH_CREATE_DIRECTORY |
        __WASI_RIGHTS_PATH_CREATE_FILE |
        __WASI_RIGHTS_PATH_UNLINK_FILE |
        __WASI_RIGHTS_PATH_REMOVE_DIRECTORY |
        __WASI_RIGHTS_PATH_SYMLINK |
        __WASI_RIGHTS_PATH_READLINK;
    __wasi_rights_t inheriting_rights =
        base_rights |
        __WASI_RIGHTS_FD_READ |
        __WASI_RIGHTS_FD_WRITE |
        __WASI_RIGHTS_FD_SEEK |
        __WASI_RIGHTS_FD_TELL |
        __WASI_RIGHTS_FD_FILESTAT_GET;
    __wasi_errno_t err = __wasi_path_open(
        (__wasi_fd_t)dir_fd,
        0,
        path,
        __WASI_OFLAGS_DIRECTORY,
        base_rights,
        inheriting_rights,
        0,
        &out_fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    return out_fd;
}

static void remove_path_if_exists(int dir_fd, const char *path)
{
    __wasi_filestat_t stat;
    __wasi_errno_t err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, path, &stat);
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

static void create_dir(int dir_fd, const char *path)
{
    __wasi_errno_t err = __wasi_path_create_directory((__wasi_fd_t)dir_fd, path);
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void create_file_with_data(int dir_fd, const char *path, const char *data)
{
    __wasi_fd_t fd = 0;
    __wasi_rights_t rights =
        __WASI_RIGHTS_FD_READ | __WASI_RIGHTS_FD_WRITE |
        __WASI_RIGHTS_FD_SEEK | __WASI_RIGHTS_FD_TELL |
        __WASI_RIGHTS_FD_FILESTAT_GET;
    __wasi_errno_t err = __wasi_path_open(
        (__wasi_fd_t)dir_fd,
        0,
        path,
        __WASI_OFLAGS_CREAT | __WASI_OFLAGS_TRUNC,
        rights,
        rights,
        0,
        &fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    if (data != NULL) {
        size_t len = strlen(data);
        assert(write((int)fd, data, len) == (ssize_t)len);
    }
    assert(close((int)fd) == 0);
}

static void create_file(int dir_fd, const char *path)
{
    create_file_with_data(dir_fd, path, NULL);
}

static __wasi_errno_t wasi_path_rename(int dir_fd, const char *old_path, const char *new_path)
{
    return __wasi_path_rename(
        (__wasi_fd_t)dir_fd,
        old_path,
        (__wasi_fd_t)dir_fd,
        new_path);
}

static void assert_path_missing(int dir_fd, const char *path)
{
    __wasi_filestat_t stat;
    __wasi_errno_t err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, path, &stat);
    assert(err == __WASI_ERRNO_NOENT);
}

static void assert_path_type(int dir_fd, const char *path, __wasi_filetype_t expected)
{
    __wasi_filestat_t stat;
    __wasi_errno_t err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, path, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == expected);
}

static __wasi_filestat_t get_filestat(int dir_fd, const char *path)
{
    __wasi_filestat_t stat;
    __wasi_errno_t err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, path, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    return stat;
}

static void test_dir_rename_nonexistent(int dir_fd)
{
    printf("Test 1: rename directory to non-existent path\n");
    remove_path_if_exists(dir_fd, "pr_dir_src");
    remove_path_if_exists(dir_fd, "pr_dir_dst");

    create_dir(dir_fd, "pr_dir_src");
    __wasi_filestat_t before = get_filestat(dir_fd, "pr_dir_src");

    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_dir_src", "pr_dir_dst");
    assert(err == __WASI_ERRNO_SUCCESS);

    assert_path_missing(dir_fd, "pr_dir_src");
    assert_path_type(dir_fd, "pr_dir_dst", __WASI_FILETYPE_DIRECTORY);

    __wasi_filestat_t after = get_filestat(dir_fd, "pr_dir_dst");
    assert(before.ino == after.ino);

    remove_path_if_exists(dir_fd, "pr_dir_dst");
}

static void test_dir_rename_over_empty(int dir_fd)
{
    printf("Test 2: rename directory over empty directory\n");
    remove_path_if_exists(dir_fd, "pr_dir_src");
    remove_path_if_exists(dir_fd, "pr_dir_dst");

    create_dir(dir_fd, "pr_dir_src");
    create_dir(dir_fd, "pr_dir_dst");

    __wasi_filestat_t before = get_filestat(dir_fd, "pr_dir_src");
    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_dir_src", "pr_dir_dst");
    assert(err == __WASI_ERRNO_SUCCESS);

    assert_path_missing(dir_fd, "pr_dir_src");
    __wasi_filestat_t after = get_filestat(dir_fd, "pr_dir_dst");
    assert(before.ino == after.ino);

    remove_path_if_exists(dir_fd, "pr_dir_dst");
}

static void test_dir_rename_over_nonempty(int dir_fd)
{
    printf("Test 3: rename directory over non-empty directory fails\n");
    remove_path_if_exists(dir_fd, "pr_dir_src");
    remove_path_if_exists(dir_fd, "pr_dir_dst");
    remove_path_if_exists(dir_fd, "pr_dir_dst/file");

    create_dir(dir_fd, "pr_dir_src");
    create_dir(dir_fd, "pr_dir_dst");
    create_file(dir_fd, "pr_dir_dst/file");

    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_dir_src", "pr_dir_dst");
    assert(err == __WASI_ERRNO_NOTEMPTY || err == __WASI_ERRNO_EXIST);

    remove_path_if_exists(dir_fd, "pr_dir_src");
    remove_path_if_exists(dir_fd, "pr_dir_dst/file");
    remove_path_if_exists(dir_fd, "pr_dir_dst");
}

static void test_dir_rename_over_file(int dir_fd)
{
    printf("Test 4: rename directory over file fails with NOTDIR\n");
    remove_path_if_exists(dir_fd, "pr_dir_src");
    remove_path_if_exists(dir_fd, "pr_file_dst");

    create_dir(dir_fd, "pr_dir_src");
    create_file(dir_fd, "pr_file_dst");

    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_dir_src", "pr_file_dst");
    expect_errno(err, __WASI_ERRNO_NOTDIR, "dir over file should be NOTDIR");

    remove_path_if_exists(dir_fd, "pr_dir_src");
    remove_path_if_exists(dir_fd, "pr_file_dst");
}

static void test_dir_rename_to_own_child(int dir_fd)
{
    printf("Test 5: rename directory into its own child fails with INVAL\n");
    remove_path_if_exists(dir_fd, "pr_dir_src");
    remove_path_if_exists(dir_fd, "pr_dir_src/child");

    create_dir(dir_fd, "pr_dir_src");
    create_dir(dir_fd, "pr_dir_src/child");

    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_dir_src", "pr_dir_src/child");
    expect_errno(err, __WASI_ERRNO_INVAL, "dir to its own child should be INVAL");

    remove_path_if_exists(dir_fd, "pr_dir_src/child");
    remove_path_if_exists(dir_fd, "pr_dir_src");
}

static void test_file_rename_nonexistent(int dir_fd)
{
    printf("Test 6: rename file to non-existent path\n");
    remove_path_if_exists(dir_fd, "pr_file_src");
    remove_path_if_exists(dir_fd, "pr_file_dst");

    create_file_with_data(dir_fd, "pr_file_src", "hello");
    __wasi_filestat_t before = get_filestat(dir_fd, "pr_file_src");

    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_file_src", "pr_file_dst");
    assert(err == __WASI_ERRNO_SUCCESS);

    assert_path_missing(dir_fd, "pr_file_src");
    assert_path_type(dir_fd, "pr_file_dst", __WASI_FILETYPE_REGULAR_FILE);

    __wasi_filestat_t after = get_filestat(dir_fd, "pr_file_dst");
    assert(before.ino == after.ino);

    remove_path_if_exists(dir_fd, "pr_file_dst");
}

static void test_file_rename_over_file(int dir_fd)
{
    printf("Test 7: rename file over existing file replaces contents\n");
    remove_path_if_exists(dir_fd, "pr_file_src");
    remove_path_if_exists(dir_fd, "pr_file_dst");

    create_file_with_data(dir_fd, "pr_file_src", "first");
    create_file_with_data(dir_fd, "pr_file_dst", "second");

    __wasi_filestat_t before = get_filestat(dir_fd, "pr_file_src");
    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_file_src", "pr_file_dst");
    assert(err == __WASI_ERRNO_SUCCESS);

    assert_path_missing(dir_fd, "pr_file_src");
    __wasi_filestat_t after = get_filestat(dir_fd, "pr_file_dst");
    assert(before.ino == after.ino);

    __wasi_fd_t fd = 0;
    __wasi_rights_t rights = __WASI_RIGHTS_FD_READ | __WASI_RIGHTS_FD_SEEK | __WASI_RIGHTS_FD_TELL;
    err = __wasi_path_open(
        (__wasi_fd_t)dir_fd,
        0,
        "pr_file_dst",
        0,
        rights,
        rights,
        0,
        &fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    char buf[8] = {0};
    assert(read((int)fd, buf, sizeof(buf)) >= 0);
    assert(close((int)fd) == 0);
    assert(strncmp(buf, "first", 5) == 0);

    remove_path_if_exists(dir_fd, "pr_file_dst");
}

static void test_file_rename_over_dir(int dir_fd)
{
    printf("Test 8: rename file over directory fails with ISDIR\n");
    remove_path_if_exists(dir_fd, "pr_file_src");
    remove_path_if_exists(dir_fd, "pr_dir_dst");

    create_file(dir_fd, "pr_file_src");
    create_dir(dir_fd, "pr_dir_dst");

    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_file_src", "pr_dir_dst");
    expect_errno(err, __WASI_ERRNO_ISDIR, "file over dir should be ISDIR");

    remove_path_if_exists(dir_fd, "pr_file_src");
    remove_path_if_exists(dir_fd, "pr_dir_dst");
}

static void test_rename_self(int dir_fd)
{
    printf("Test 9: rename file and dir to self succeeds\n");
    remove_path_if_exists(dir_fd, "pr_self_file");
    remove_path_if_exists(dir_fd, "pr_self_dir");

    create_file(dir_fd, "pr_self_file");
    create_dir(dir_fd, "pr_self_dir");

    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_self_file", "pr_self_file");
    assert(err == __WASI_ERRNO_SUCCESS);
    err = wasi_path_rename(dir_fd, "pr_self_dir", "pr_self_dir");
    assert(err == __WASI_ERRNO_SUCCESS);

    remove_path_if_exists(dir_fd, "pr_self_file");
    remove_path_if_exists(dir_fd, "pr_self_dir");
}

static void test_file_missing(int dir_fd)
{
    printf("Test 10: rename missing source fails with NOENT\n");
    remove_path_if_exists(dir_fd, "pr_missing_src");
    remove_path_if_exists(dir_fd, "pr_missing_dst");

    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_missing_src", "pr_missing_dst");
    expect_errno(err, __WASI_ERRNO_NOENT, "missing source should be NOENT");
}

static void test_trailing_slashes(int dir_fd)
{
    printf("Test 11: trailing slashes on directory names\n");
    remove_path_if_exists(dir_fd, "pr_ts_source");
    remove_path_if_exists(dir_fd, "pr_ts_target");

    create_dir(dir_fd, "pr_ts_source");

    __wasi_errno_t err = __wasi_path_rename(
        (__wasi_fd_t)dir_fd,
        "pr_ts_source/",
        (__wasi_fd_t)dir_fd,
        "pr_ts_target");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_rename(
        (__wasi_fd_t)dir_fd,
        "pr_ts_target",
        (__wasi_fd_t)dir_fd,
        "pr_ts_source/");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_rename(
        (__wasi_fd_t)dir_fd,
        "pr_ts_source/",
        (__wasi_fd_t)dir_fd,
        "pr_ts_target/");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = wasi_path_rename(dir_fd, "pr_ts_target", "pr_ts_source");
    assert(err == __WASI_ERRNO_SUCCESS);

    remove_path_if_exists(dir_fd, "pr_ts_source");
}

static void test_symlink_rename(int dir_fd)
{
    printf("Test 12: rename symlink preserves link and target\n");
    remove_path_if_exists(dir_fd, "pr_link_old");
    remove_path_if_exists(dir_fd, "pr_link_new");
    remove_path_if_exists(dir_fd, "pr_link_target");

    __wasi_errno_t err = __wasi_path_symlink("pr_link_target", (__wasi_fd_t)dir_fd, "pr_link_old");
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filestat_t stat;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pr_link_old", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_SYMBOLIC_LINK);

    err = wasi_path_rename(dir_fd, "pr_link_old", "pr_link_new");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pr_link_new", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_SYMBOLIC_LINK);

    uint8_t buf[64];
    __wasi_size_t used = 0;
    err = __wasi_path_readlink((__wasi_fd_t)dir_fd, "pr_link_new", buf, sizeof(buf), &used);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(used == strlen("pr_link_target"));
    assert(memcmp(buf, "pr_link_target", used) == 0);

    remove_path_if_exists(dir_fd, "pr_link_new");
}

static void test_symlink_rename_dangling(int dir_fd)
{
    printf("Test 13: rename dangling symlink keeps dangling target\n");
    remove_path_if_exists(dir_fd, "pr_dangling_old");
    remove_path_if_exists(dir_fd, "pr_dangling_new");

    __wasi_errno_t err = __wasi_path_symlink("missing_target", (__wasi_fd_t)dir_fd, "pr_dangling_old");
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_filestat_t stat;
    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pr_dangling_old", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_SYMBOLIC_LINK);

    err = wasi_path_rename(dir_fd, "pr_dangling_old", "pr_dangling_new");
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_path_filestat_get((__wasi_fd_t)dir_fd, 0, "pr_dangling_new", &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_SYMBOLIC_LINK);

    err = __wasi_path_filestat_get(
        (__wasi_fd_t)dir_fd,
        __WASI_LOOKUPFLAGS_SYMLINK_FOLLOW,
        "pr_dangling_new",
        &stat);
    assert(err == __WASI_ERRNO_NOENT);

    remove_path_if_exists(dir_fd, "pr_dangling_new");
}

static void test_open_fd_after_rename(int dir_fd)
{
    printf("Test 14: open fd remains usable after rename\n");
    remove_path_if_exists(dir_fd, "pr_open_old");
    remove_path_if_exists(dir_fd, "pr_open_new");

    create_file_with_data(dir_fd, "pr_open_old", "abcdef");
    __wasi_fd_t fd_wasi = 0;
    __wasi_rights_t rights =
        __WASI_RIGHTS_FD_READ |
        __WASI_RIGHTS_FD_FILESTAT_GET;
    __wasi_errno_t err = __wasi_path_open(
        (__wasi_fd_t)dir_fd,
        0,
        "pr_open_old",
        0,
        rights,
        rights,
        0,
        &fd_wasi);
    assert(err == __WASI_ERRNO_SUCCESS);
    int fd = (int)fd_wasi;
    assert(fd >= 0);

    err = wasi_path_rename(dir_fd, "pr_open_old", "pr_open_new");
    assert(err == __WASI_ERRNO_SUCCESS);

    char buf[8] = {0};
    assert(read(fd, buf, sizeof(buf)) >= 0);
    assert(close(fd) == 0);
    assert(strncmp(buf, "abcdef", 6) == 0);

    remove_path_if_exists(dir_fd, "pr_open_new");
}

static void test_cross_dir_rename(int dir_fd)
{
    printf("Test 15: rename across directories\n");
    remove_path_if_exists(dir_fd, "pr_parent");
    remove_path_if_exists(dir_fd, "pr_parent/child1");
    remove_path_if_exists(dir_fd, "pr_parent/child2");
    remove_path_if_exists(dir_fd, "pr_parent/child1/file");
    remove_path_if_exists(dir_fd, "pr_parent/child2/file");

    create_dir(dir_fd, "pr_parent");
    create_dir(dir_fd, "pr_parent/child1");
    create_dir(dir_fd, "pr_parent/child2");
    create_file_with_data(dir_fd, "pr_parent/child1/file", "move");

    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_parent/child1/file", "pr_parent/child2/file");
    assert(err == __WASI_ERRNO_SUCCESS);

    assert_path_missing(dir_fd, "pr_parent/child1/file");
    assert_path_type(dir_fd, "pr_parent/child2/file", __WASI_FILETYPE_REGULAR_FILE);

    remove_path_if_exists(dir_fd, "pr_parent/child2/file");
    remove_path_if_exists(dir_fd, "pr_parent/child1");
    remove_path_if_exists(dir_fd, "pr_parent/child2");
    remove_path_if_exists(dir_fd, "pr_parent");
}

static void test_name_too_long(int dir_fd)
{
    printf("Test 16: rename with too-long name\n");
    remove_path_if_exists(dir_fd, "pr_long_src");
    create_file(dir_fd, "pr_long_src");

    size_t name_len = (size_t)NAME_MAX + 1;
    char *long_name = (char *)malloc(name_len + 1);
    assert(long_name != NULL);
    memset(long_name, 'a', name_len);
    long_name[name_len] = '\0';

    __wasi_errno_t err = __wasi_path_rename(
        (__wasi_fd_t)dir_fd,
        "pr_long_src",
        (__wasi_fd_t)dir_fd,
        long_name);
    expect_errno(err, __WASI_ERRNO_NAMETOOLONG, "rename with long name");

    free(long_name);
    remove_path_if_exists(dir_fd, "pr_long_src");
}

static void test_invalid_pointer(int dir_fd)
{
    printf("Test 17: invalid pointer returns MEMVIOLATION\n");
    const char *bad_ptr = (const char *)0xFFFFFFFFu;
    __wasi_size_t bad_len = 1;
    __wasi_size_t valid_len = (__wasi_size_t)strlen("pr_invalid");
    __wasi_errno_t err = __wasi_path_rename_raw(
        (__wasi_fd_t)dir_fd,
        bad_ptr,
        bad_len,
        (__wasi_fd_t)dir_fd,
        "pr_invalid",
        valid_len);
    expect_errno(err, __WASI_ERRNO_MEMVIOLATION, "invalid old path pointer");

    err = __wasi_path_rename_raw(
        (__wasi_fd_t)dir_fd,
        "pr_invalid",
        valid_len,
        (__wasi_fd_t)dir_fd,
        bad_ptr,
        bad_len);
    expect_errno(err, __WASI_ERRNO_MEMVIOLATION, "invalid new path pointer");
}

static void test_dot_paths(int dir_fd)
{
    printf("Test 18: paths with '.' or '..' should fail with BUSY\n");
    remove_path_if_exists(dir_fd, "pr_dot_src");
    remove_path_if_exists(dir_fd, "pr_dot_dst");

    create_dir(dir_fd, "pr_dot_src");
    create_dir(dir_fd, "pr_dot_dst");

    __wasi_errno_t err = wasi_path_rename(dir_fd, "pr_dot_src/.", "pr_dot_dst");
    expect_errno(err, __WASI_ERRNO_BUSY, "rename dir/. to dir should be BUSY");

    err = wasi_path_rename(dir_fd, "pr_dot_src/..", "pr_dot_dst");
    expect_errno(err, __WASI_ERRNO_BUSY, "rename dir/.. to dir should be BUSY");

    err = wasi_path_rename(dir_fd, "pr_dot_src", "pr_dot_dst/.");
    expect_errno(err, __WASI_ERRNO_BUSY, "rename dir to dir/. should be BUSY");

    err = wasi_path_rename(dir_fd, "pr_dot_src", "pr_dot_dst/..");
    expect_errno(err, __WASI_ERRNO_BUSY, "rename dir to dir/.. should be BUSY");

    remove_path_if_exists(dir_fd, "pr_dot_src");
    remove_path_if_exists(dir_fd, "pr_dot_dst");
}

int main(void)
{
    int preopen_fd = find_preopen_fd();
    assert(preopen_fd >= 0);

    remove_path_if_exists(preopen_fd, "pr_root");
    create_dir(preopen_fd, "pr_root");

    __wasi_fd_t dir_fd = open_dir_fd(preopen_fd, "pr_root");

    test_dir_rename_nonexistent(dir_fd);
    test_dir_rename_over_empty(dir_fd);
    test_dir_rename_over_nonempty(dir_fd);
    test_dir_rename_over_file(dir_fd);
    test_dir_rename_to_own_child(dir_fd);
    test_file_rename_nonexistent(dir_fd);
    test_file_rename_over_file(dir_fd);
    test_file_rename_over_dir(dir_fd);
    test_rename_self(dir_fd);
    test_file_missing(dir_fd);
    test_trailing_slashes(dir_fd);
    test_symlink_rename(dir_fd);
    test_symlink_rename_dangling(dir_fd);
    test_open_fd_after_rename(dir_fd);
    test_cross_dir_rename(dir_fd);
    test_name_too_long(dir_fd);
    test_invalid_pointer(dir_fd);
    test_dot_paths(dir_fd);

    assert(__wasi_fd_close(dir_fd) == __WASI_ERRNO_SUCCESS);
    remove_path_if_exists(preopen_fd, "pr_root");

    return 0;
}
