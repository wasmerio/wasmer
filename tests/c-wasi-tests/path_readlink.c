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

static void unlink_if_exists(int dir_fd, const char *path)
{
    __wasi_errno_t err = __wasi_path_unlink_file((__wasi_fd_t)dir_fd, path);
    if (err != __WASI_ERRNO_SUCCESS) {
        assert(err == __WASI_ERRNO_NOENT);
    }
}

static void create_file(const char *name)
{
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);
}

static void create_symlink(int dir_fd, const char *target, const char *link)
{
    __wasi_errno_t err =
        __wasi_path_symlink(target, (__wasi_fd_t)dir_fd, link);
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_basic_readlink(int dir_fd)
{
    printf("Test 1: basic readlink\n");
    const char *target = "prl_target";
    const char *link = "prl_link";
    unlink_if_exists(dir_fd, target);
    unlink_if_exists(dir_fd, link);

    create_file(target);
    create_symlink(dir_fd, target, link);

    uint8_t buf[16];
    memset(buf, 0xAA, sizeof(buf));
    __wasi_size_t used = 0;
    __wasi_errno_t err =
        __wasi_path_readlink((__wasi_fd_t)dir_fd, link, buf, sizeof(buf), &used);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(used == strlen(target));
    assert(memcmp(buf, target, used) == 0);
    for (size_t i = used; i < sizeof(buf); ++i) {
        assert(buf[i] == 0xAA);
    }

    unlink_if_exists(dir_fd, link);
    unlink_if_exists(dir_fd, target);
}

static void test_truncated_readlink(int dir_fd)
{
    printf("Test 2: readlink truncates to buffer length\n");
    const char *target = "prl_long_target";
    const char *link = "prl_link_small";
    unlink_if_exists(dir_fd, target);
    unlink_if_exists(dir_fd, link);

    create_file(target);
    create_symlink(dir_fd, target, link);

    uint8_t buf[4];
    memset(buf, 0xCC, sizeof(buf));
    __wasi_size_t used = 0;
    __wasi_errno_t err =
        __wasi_path_readlink((__wasi_fd_t)dir_fd, link, buf, sizeof(buf), &used);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(used == sizeof(buf));
    assert(memcmp(buf, target, sizeof(buf)) == 0);

    unlink_if_exists(dir_fd, link);
    unlink_if_exists(dir_fd, target);
}

static void test_incremental_readlink(int dir_fd)
{
    printf("Test 3: incremental readlink growth\n");
    const char *target =
        "\xD0\x94\xD0\xB5\xD0\xB9\xD1\x81\xD1\x82\xD0\xB2\xD0\xB8\xD0\xB5";
    const char *link = "prl_utf8_link";
    size_t target_len = strlen(target);

    unlink_if_exists(dir_fd, target);
    unlink_if_exists(dir_fd, link);

    create_file(target);
    create_symlink(dir_fd, target, link);

    int done = 0;
    for (size_t cap = 1; cap <= target_len + 2; ++cap) {
        uint8_t *buf = (uint8_t *)malloc(cap);
        assert(buf != NULL);
        memset(buf, 0, cap);

        __wasi_size_t used = 0;
        __wasi_errno_t err =
            __wasi_path_readlink((__wasi_fd_t)dir_fd, link, buf, cap, &used);
        assert(err == __WASI_ERRNO_SUCCESS);

        if (cap < target_len) {
            assert(used == cap);
            assert(memcmp(buf, target, cap) == 0);
        } else {
            assert(used == target_len);
            assert(memcmp(buf, target, target_len) == 0);
            done = 1;
        }

        free(buf);
        if (done) {
            break;
        }
    }
    assert(done);

    unlink_if_exists(dir_fd, link);
    unlink_if_exists(dir_fd, target);
}

static void test_error_cases(int dir_fd)
{
    printf("Test 4: error cases\n");
    const char *file = "prl_err_file";
    const char *link = "prl_err_link";
    unlink_if_exists(dir_fd, file);
    unlink_if_exists(dir_fd, link);

    create_file(file);
    create_symlink(dir_fd, file, link);

    uint8_t buf[8];
    __wasi_size_t used = 0;

    __wasi_errno_t err =
        __wasi_path_readlink((__wasi_fd_t)dir_fd, link, buf, 0, &used);
    assert(err == __WASI_ERRNO_INVAL);

    err = __wasi_path_readlink((__wasi_fd_t)dir_fd, file, buf, sizeof(buf), &used);
    assert(err == __WASI_ERRNO_INVAL);

    int fd = open("prl_dirfd_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    err = __wasi_path_readlink((__wasi_fd_t)fd, link, buf, sizeof(buf), &used);
    assert(err == __WASI_ERRNO_NOTDIR);
    assert(close(fd) == 0);
    unlink_if_exists(dir_fd, "prl_dirfd_file");

    err = __wasi_path_readlink((__wasi_fd_t)dir_fd, "prl_err_file/child", buf, sizeof(buf), &used);
    assert(err == __WASI_ERRNO_NOTDIR);

    err = __wasi_path_readlink((__wasi_fd_t)9999, link, buf, sizeof(buf), &used);
    assert(err == __WASI_ERRNO_BADF);

    err = __wasi_path_readlink((__wasi_fd_t)dir_fd, "prl_missing", buf, sizeof(buf), &used);
    assert(err == __WASI_ERRNO_NOENT);

    unlink_if_exists(dir_fd, link);
    unlink_if_exists(dir_fd, file);
}

int main(void)
{
    int dir_fd = find_preopen_fd();
    assert(dir_fd >= 0);

    test_basic_readlink(dir_fd);
    test_truncated_readlink(dir_fd);
    test_incremental_readlink(dir_fd);
    test_error_cases(dir_fd);

    printf("All tests passed!\n");
    return 0;
}
