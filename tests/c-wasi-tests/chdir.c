#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

__attribute__((__import_module__("wasix_32v1"), __import_name__("chdir")))
extern __wasi_errno_t wasix_chdir_raw(const char *path, __wasi_pointersize_t path_len);

#ifndef PATH_MAX
#define PATH_MAX 4096
#endif

static void make_dir(const char *name, mode_t mode)
{
    if (mkdir(name, mode) == -1) {
        assert(errno == EEXIST);
    }
}

static void test_basic_chdir_and_getcwd(const char *cwd)
{
    printf("Test 1: basic chdir + getcwd\n");
    make_dir("chdir_dir", 0755);
    assert(chdir("chdir_dir") == 0);

    char now[PATH_MAX];
    assert(getcwd(now, sizeof(now)) != NULL);

    char expected[PATH_MAX];
    int n = snprintf(expected, sizeof(expected), "%s/%s", cwd, "chdir_dir");
    assert(n > 0 && (size_t)n < sizeof(expected));
    assert(strcmp(now, expected) == 0);

    assert(chdir(".") == 0);
    char dot[PATH_MAX];
    assert(getcwd(dot, sizeof(dot)) != NULL);
    assert(strcmp(dot, expected) == 0);

    assert(chdir("..") == 0);
    char back[PATH_MAX];
    assert(getcwd(back, sizeof(back)) != NULL);
    assert(strcmp(back, cwd) == 0);

    assert(rmdir("chdir_dir") == 0);
}

static void test_chdir_affects_relative_open(void)
{
    printf("Test 2: chdir affects relative open\n");
    make_dir("chdir_data", 0755);

    int fd = open("chdir_data/inner.txt", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);

    assert(chdir("chdir_data") == 0);
    fd = open("inner.txt", O_RDONLY);
    assert(fd >= 0);
    assert(close(fd) == 0);

    assert(chdir("..") == 0);
    assert(unlink("chdir_data/inner.txt") == 0);
    assert(rmdir("chdir_data") == 0);
}

static void test_notdir(void)
{
    printf("Test 3: chdir on file (ENOTDIR)\n");
    int fd = open("chdir_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);

    errno = 0;
    assert(chdir("chdir_file") == -1);
    assert(errno == ENOTDIR);

    assert(unlink("chdir_file") == 0);
}

static void test_noent(void)
{
    printf("Test 4: chdir on missing path (ENOENT)\n");
    errno = 0;
    assert(chdir("does_not_exist") == -1);
    assert(errno == ENOENT);
}

static void test_eacces(void)
{
    printf("Test 5: chdir without execute permission (EACCES)\n");
    make_dir("chdir_noexec", 0000);
    assert(chmod("chdir_noexec", 0000) == 0);

    errno = 0;
    assert(chdir("chdir_noexec") == -1);
    assert(errno == EACCES);

    assert(chmod("chdir_noexec", 0700) == 0);
    assert(rmdir("chdir_noexec") == 0);
}

static void test_enametoolong(void)
{
    printf("Test 6: chdir with long component (ENAMETOOLONG)\n");
    char longname[300];
    memset(longname, 'a', sizeof(longname) - 1);
    longname[sizeof(longname) - 1] = '\0';

    __wasi_errno_t err = wasix_chdir_raw(longname, sizeof(longname) - 1);
    assert(err == __WASI_ERRNO_NAMETOOLONG);
}

static void test_efault(void)
{
    printf("Test 7: chdir with invalid pointer (MEMVIOLATION)\n");
    const char *bad = (const char *)(uintptr_t)0xffffffffu;
    __wasi_errno_t err = wasix_chdir_raw(bad, 1);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

static void test_symlink_loop(void)
{
    printf("Test 8: chdir symlink loop (ELOOP)\n");
    errno = 0;
    if (symlink("symloop2", "symloop1") != 0) {
        assert(errno == EPERM || errno == ENOSYS);
        printf("  Skipping symlink loop test (symlink unsupported)\n");
        return;
    }
    assert(symlink("symloop1", "symloop2") == 0);

    errno = 0;
    assert(chdir("symloop1") == -1);
    assert(errno == ELOOP);

    assert(unlink("symloop1") == 0);
    assert(unlink("symloop2") == 0);
}

int main(void)
{
    char cwd[PATH_MAX];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    test_basic_chdir_and_getcwd(cwd);
    test_chdir_affects_relative_open();
    test_notdir();
    test_noent();
    test_symlink_loop();
    test_efault();
    test_enametoolong();
    test_eacces();

    printf("All tests passed!\n");
    return 0;
}
