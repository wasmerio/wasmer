#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasi.h>
#include <wasi/api_wasix.h>

static int find_preopen_fd(void)
{
    int fallback_fd = -1;
    for (int fd = 4; fd < 64; ++fd) {
        __wasi_prestat_t prestat;
        __wasi_errno_t err = __wasi_fd_prestat_get((__wasi_fd_t)fd, &prestat);
        if (err == __WASI_ERRNO_SUCCESS && prestat.tag == __WASI_PREOPENTYPE_DIR) {
            __wasi_size_t len = prestat.u.dir.pr_name_len;
            char name[256] = {0};
            if (len >= sizeof(name)) {
                continue;
            }
            err = __wasi_fd_prestat_dir_name((__wasi_fd_t)fd, (uint8_t *)name, len);
            assert(err == __WASI_ERRNO_SUCCESS);
            name[len] = '\0';
            if (strcmp(name, ".") == 0) {
                return fd;
            }
            if (fallback_fd == -1 && strcmp(name, "/dev") != 0) {
                fallback_fd = fd;
            }
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

static __wasi_errno_t path_open2_at(int dir_fd, const char *path, __wasi_oflags_t oflags,
                                   __wasi_rights_t rights, __wasi_fdflags_t fdflags,
                                   __wasi_fd_t *out)
{
    return __wasi_path_open2((__wasi_fd_t)dir_fd, __WASI_LOOKUPFLAGS_SYMLINK_FOLLOW, path, oflags, rights, rights, fdflags, 0,
                             out);
}

static void test_basic_create_read(int dir_fd)
{
    printf("Test 1: basic create + read\n");
    remove_path_if_exists(dir_fd, "po2_file");

    __wasi_fd_t fd = 0;
    __wasi_rights_t rights = __WASI_RIGHTS_FD_READ | __WASI_RIGHTS_FD_WRITE |
                             __WASI_RIGHTS_FD_SEEK | __WASI_RIGHTS_FD_TELL |
                             __WASI_RIGHTS_FD_FILESTAT_GET;

    __wasi_errno_t err = path_open2_at(dir_fd, "po2_file",
                                      __WASI_OFLAGS_CREAT | __WASI_OFLAGS_TRUNC,
                                      rights, 0, &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    const char *msg = "abc";
    __wasi_ciovec_t iov = {.buf = (uint8_t *)msg, .buf_len = 3};
    __wasi_size_t nw = 0;
    err = __wasi_fd_write(fd, &iov, 1, &nw);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nw == 3);
    assert(__wasi_fd_close(fd) == __WASI_ERRNO_SUCCESS);

    fd = 0;
    err = path_open2_at(dir_fd, "po2_file", 0, __WASI_RIGHTS_FD_READ, 0, &fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    char buf[4] = {0};
    __wasi_iovec_t riov = {.buf = (uint8_t *)buf, .buf_len = 3};
    __wasi_size_t nr = 0;
    err = __wasi_fd_read(fd, &riov, 1, &nr);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nr == 3);
    assert(memcmp(buf, "abc", 3) == 0);
    assert(__wasi_fd_close(fd) == __WASI_ERRNO_SUCCESS);

    remove_path_if_exists(dir_fd, "po2_file");
}

static void test_excl_existing(int dir_fd)
{
    printf("Test 2: O_EXCL on existing -> EEXIST\n");
    remove_path_if_exists(dir_fd, "po2_excl");

    __wasi_fd_t fd = 0;
    __wasi_errno_t err = path_open2_at(
        dir_fd, "po2_excl", __WASI_OFLAGS_CREAT, __WASI_RIGHTS_FD_READ, 0, &fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(fd) == __WASI_ERRNO_SUCCESS);

    fd = 0;
    err = path_open2_at(dir_fd, "po2_excl",
                        __WASI_OFLAGS_CREAT | __WASI_OFLAGS_EXCL,
                        __WASI_RIGHTS_FD_READ, 0, &fd);
    assert(err == __WASI_ERRNO_EXIST);

    remove_path_if_exists(dir_fd, "po2_excl");
}

static void test_invalid_dirfd(void)
{
    printf("Test 3: invalid dirfd -> EBADF\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = path_open2_at(-1, "po2_badfd", 0, __WASI_RIGHTS_FD_READ, 0, &fd);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_dirfd_is_file(int dir_fd)
{
    printf("Test 4: dirfd is file -> ENOTDIR\n");
    remove_path_if_exists(dir_fd, "po2_dirfd_file");

    __wasi_fd_t filefd = 0;
    __wasi_errno_t err = path_open2_at(
        dir_fd, "po2_dirfd_file", __WASI_OFLAGS_CREAT, __WASI_RIGHTS_FD_READ, 0, &filefd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_fd_t fd = 0;
    err = path_open2_at((int)filefd, "child", 0, __WASI_RIGHTS_FD_READ, 0, &fd);
    assert(err == __WASI_ERRNO_NOTDIR);

    assert(__wasi_fd_close(filefd) == __WASI_ERRNO_SUCCESS);
    remove_path_if_exists(dir_fd, "po2_dirfd_file");
}

static void test_open_directory_flag_on_file(int dir_fd)
{
    printf("Test 5: O_DIRECTORY on file -> ENOTDIR\n");
    remove_path_if_exists(dir_fd, "po2_dirflag");

    __wasi_fd_t fd = 0;
    __wasi_errno_t err = path_open2_at(
        dir_fd, "po2_dirflag", __WASI_OFLAGS_CREAT, __WASI_RIGHTS_FD_READ, 0, &fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(fd) == __WASI_ERRNO_SUCCESS);

    fd = 0;
    err = path_open2_at(dir_fd, "po2_dirflag", __WASI_OFLAGS_DIRECTORY,
                        __WASI_RIGHTS_FD_READ, 0, &fd);
    assert(err == __WASI_ERRNO_NOTDIR);

    remove_path_if_exists(dir_fd, "po2_dirflag");
}

static void test_empty_path(int dir_fd)
{
    printf("Test 6: empty path -> ENOENT\n");
    __wasi_fd_t fd = 0;
    __wasi_errno_t err = path_open2_at(dir_fd, "", 0, __WASI_RIGHTS_FD_READ, 0, &fd);
    assert(err == __WASI_ERRNO_NOENT);
}

static void test_trunc_resets_size(int dir_fd)
{
    printf("Test 7: O_TRUNC resets size to 0\n");
    remove_path_if_exists(dir_fd, "po2_trunc");

    __wasi_fd_t fd = 0;
    __wasi_rights_t rights = __WASI_RIGHTS_FD_READ | __WASI_RIGHTS_FD_WRITE |
                             __WASI_RIGHTS_FD_FILESTAT_GET;

    __wasi_errno_t err = path_open2_at(
        dir_fd, "po2_trunc", __WASI_OFLAGS_CREAT, rights, 0, &fd);
    assert(err == __WASI_ERRNO_SUCCESS);

    const char *msg = "abcdef";
    __wasi_ciovec_t iov = {.buf = (uint8_t *)msg, .buf_len = 6};
    __wasi_size_t nw = 0;
    err = __wasi_fd_write(fd, &iov, 1, &nw);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(nw == 6);
    assert(__wasi_fd_close(fd) == __WASI_ERRNO_SUCCESS);

    fd = 0;
    err = path_open2_at(dir_fd, "po2_trunc", __WASI_OFLAGS_TRUNC, rights, 0, &fd);
    assert(err == __WASI_ERRNO_SUCCESS);
    __wasi_filestat_t st;
    err = __wasi_fd_filestat_get(fd, &st);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(st.size == 0);
    assert(__wasi_fd_close(fd) == __WASI_ERRNO_SUCCESS);

    remove_path_if_exists(dir_fd, "po2_trunc");
}

int main(void)
{
    int dir_fd = find_preopen_fd();
    assert(dir_fd >= 0);

    test_basic_create_read(dir_fd);
    test_excl_existing(dir_fd);
    test_invalid_dirfd();
    test_dirfd_is_file(dir_fd);
    test_open_directory_flag_on_file(dir_fd);
    test_empty_path(dir_fd);
    test_trunc_resets_size(dir_fd);
    return 0;
}
