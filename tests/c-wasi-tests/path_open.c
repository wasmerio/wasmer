#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <dirent.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

static void unlink_if_exists(const char *path)
{
    struct stat st;
    if (lstat(path, &st) == 0) {
        if (S_ISDIR(st.st_mode)) {
            (void)rmdir(path);
        } else {
            (void)unlink(path);
        }
    }
}

static void remove_tree(const char *path)
{
    DIR *dir = opendir(path);
    if (!dir) {
        unlink_if_exists(path);
        return;
    }

    struct dirent *ent;
    while ((ent = readdir(dir)) != NULL) {
        if (strcmp(ent->d_name, ".") == 0 || strcmp(ent->d_name, "..") == 0) {
            continue;
        }
        char child[PATH_MAX];
        snprintf(child, sizeof(child), "%s/%s", path, ent->d_name);
        struct stat st;
        if (lstat(child, &st) == 0 && S_ISDIR(st.st_mode)) {
            remove_tree(child);
        } else {
            (void)unlink(child);
        }
    }
    closedir(dir);
    (void)rmdir(path);
}

static void ensure_clean_dir(const char *path)
{
    remove_tree(path);
    assert(mkdir(path, 0700) == 0);
}

static void write_all(int fd, const char *buf, size_t len)
{
    size_t off = 0;
    while (off < len) {
        ssize_t n = write(fd, buf + off, len - off);
        assert(n > 0);
        off += (size_t)n;
    }
}

static void read_all(int fd, char *buf, size_t len)
{
    size_t off = 0;
    while (off < len) {
        ssize_t n = read(fd, buf + off, len - off);
        assert(n > 0);
        off += (size_t)n;
    }
}

static void test_relative_dirfd(int dirfd)
{
    printf("Test 1: relative path uses dirfd\n");
    unlink_if_exists("t_rel");
    int fd = openat(dirfd, "t_rel", O_CREAT | O_TRUNC | O_RDWR, 0600);
    assert(fd >= 0);
    write_all(fd, "abc", 3);
    assert(close(fd) == 0);
    fd = openat(dirfd, "t_rel", O_RDONLY, 0);
    assert(fd >= 0);
    char buf[4] = {0};
    read_all(fd, buf, 3);
    assert(strcmp(buf, "abc") == 0);
    assert(close(fd) == 0);
    unlink_if_exists("t_rel");
}

static void test_absolute_ignores_dirfd(int dirfd, const char *abs_path)
{
    printf("Test 2: absolute path ignores dirfd\n");
    int fd = openat(dirfd, abs_path, O_RDONLY, 0);
    assert(fd >= 0);
    assert(close(fd) == 0);
}

static void test_dirfd_is_file(void)
{
    printf("Test 3: dirfd is a file -> ENOTDIR\n");
    unlink_if_exists("t_filefd");
    int filefd = open("t_filefd", O_CREAT | O_TRUNC | O_RDWR, 0600);
    assert(filefd >= 0);
    errno = 0;
    int fd = openat(filefd, "child", O_RDONLY, 0);
    assert(fd == -1);
    assert(errno == ENOTDIR);
    assert(close(filefd) == 0);
    unlink_if_exists("t_filefd");
}

static void test_invalid_dirfd(void)
{
    printf("Test 4: invalid dirfd -> EBADF\n");
    errno = 0;
    int fd = openat(99999, "child", O_RDONLY, 0);
    assert(fd == -1);
    assert(errno == EBADF);
}

static void test_at_fdcwd(void)
{
    printf("Test 5: AT_FDCWD uses cwd\n");
    unlink_if_exists("t_cwd");
    int fd = openat(AT_FDCWD, "t_cwd", O_CREAT | O_TRUNC | O_RDWR, 0600);
    assert(fd >= 0);
    assert(close(fd) == 0);
    unlink_if_exists("t_cwd");
}

static void test_o_append(int dirfd)
{
    printf("Test 6: O_APPEND writes at end\n");
    unlink_if_exists("t_append");
    int fd = openat(dirfd, "t_append", O_CREAT | O_TRUNC | O_RDWR | O_APPEND, 0600);
    assert(fd >= 0);
    write_all(fd, "abc", 3);
    assert(lseek(fd, 0, SEEK_SET) == 0);
    write_all(fd, "d", 1);
    assert(close(fd) == 0);

    fd = openat(dirfd, "t_append", O_RDONLY, 0);
    assert(fd >= 0);
    char buf[5] = {0};
    read_all(fd, buf, 4);
    assert(strcmp(buf, "abcd") == 0);
    assert(close(fd) == 0);
    unlink_if_exists("t_append");
}

static void test_o_cloexec(int dirfd)
{
    printf("Test 7: O_CLOEXEC sets FD_CLOEXEC\n");
    unlink_if_exists("t_cloexec");
    int fd = openat(dirfd, "t_cloexec", O_CREAT | O_TRUNC | O_RDWR | O_CLOEXEC, 0600);
    assert(fd >= 0);
    int flags = fcntl(fd, F_GETFD);
    assert(flags >= 0);
    assert((flags & FD_CLOEXEC) != 0);
    assert(close(fd) == 0);
    unlink_if_exists("t_cloexec");
}

static void test_o_largefile(int dirfd)
{
    printf("Test 8: O_LARGEFILE accepted\n");
    unlink_if_exists("t_largefile");
#ifdef O_LARGEFILE
    int fd = openat(dirfd, "t_largefile", O_CREAT | O_TRUNC | O_RDWR | O_LARGEFILE, 0600);
    assert(fd >= 0);
    assert(close(fd) == 0);
#endif
    unlink_if_exists("t_largefile");
}

static void test_o_noatime(int dirfd)
{
    printf("Test 9: O_NOATIME does not update atime\n");
    unlink_if_exists("t_noatime");
    int fd = openat(dirfd, "t_noatime", O_CREAT | O_TRUNC | O_RDWR, 0600);
    assert(fd >= 0);
    write_all(fd, "x", 1);
    assert(close(fd) == 0);

    struct stat st_before;
    assert(stat("t_noatime", &st_before) == 0);
    sleep(1);

#ifdef O_NOATIME
    fd = openat(dirfd, "t_noatime", O_RDONLY | O_NOATIME, 0);
    assert(fd >= 0);
#else
    printf("  O_NOATIME not defined, skipping\n");
    unlink_if_exists("t_noatime");
    return;
#endif
    char buf;
    read_all(fd, &buf, 1);
    assert(close(fd) == 0);

    struct stat st_after;
    assert(stat("t_noatime", &st_after) == 0);
    assert(st_before.st_atime == st_after.st_atime);
    unlink_if_exists("t_noatime");
}

static void test_o_nofollow(int dirfd)
{
    printf("Test 10: O_NOFOLLOW on symlink -> ELOOP\n");
    unlink_if_exists("t_target");
    unlink_if_exists("t_symlink");
    int fd = openat(dirfd, "t_target", O_CREAT | O_TRUNC | O_RDWR, 0600);
    assert(fd >= 0);
    assert(close(fd) == 0);
    assert(symlink("t_target", "t_symlink") == 0);

#ifdef O_NOFOLLOW
    errno = 0;
    fd = openat(dirfd, "t_symlink", O_RDONLY | O_NOFOLLOW, 0);
    assert(fd == -1);
    assert(errno == ELOOP);
#else
    printf("  O_NOFOLLOW not defined, skipping\n");
#endif

    unlink_if_exists("t_symlink");
    unlink_if_exists("t_target");
}

static void test_o_trunc(int dirfd)
{
    printf("Test 11: O_TRUNC sets size to 0\n");
    unlink_if_exists("t_trunc");
    int fd = openat(dirfd, "t_trunc", O_CREAT | O_TRUNC | O_RDWR, 0600);
    assert(fd >= 0);
    write_all(fd, "abc", 3);
    assert(close(fd) == 0);

    fd = openat(dirfd, "t_trunc", O_TRUNC | O_RDWR, 0600);
    assert(fd >= 0);
    struct stat st;
    assert(fstat(fd, &st) == 0);
    assert(st.st_size == 0);
    assert(close(fd) == 0);
    unlink_if_exists("t_trunc");
}

static void link_tmpfile(int dirfd, int fd, const char *name)
{
    char proc_path[PATH_MAX];
    snprintf(proc_path, sizeof(proc_path), "/proc/self/fd/%d", fd);
    assert(linkat(AT_FDCWD, proc_path, dirfd, name, AT_SYMLINK_FOLLOW) == 0);
}

static void test_o_tmpfile_basic(int dirfd)
{
    printf("Test 12: O_TMPFILE basic create + link\n");
#ifndef O_TMPFILE
    printf("  O_TMPFILE not defined, skipping\n");
#else
    unlink_if_exists("tmpfile_basic");
    int fd = openat(dirfd, ".", O_TMPFILE | O_RDWR, 0600);
    assert(fd >= 0);
    write_all(fd, "abcdef", 6);
    struct stat st;
    assert(fstat(fd, &st) == 0);
    assert(st.st_size == 6);

    link_tmpfile(dirfd, fd, "tmpfile_basic");
    assert(stat("tmpfile_basic", &st) == 0);
    assert(st.st_size == 6);

    assert(close(fd) == 0);
    unlink_if_exists("tmpfile_basic");
#endif
}

static void test_o_tmpfile_multi_dirs(int dirfd)
{
    printf("Test 13: O_TMPFILE across directories\n");
#ifndef O_TMPFILE
    printf("  O_TMPFILE not defined, skipping\n");
#else
    const char *dirs[] = {"tmpdir1", "tmpdir2", "tmpdir3"};
    int fds[3] = {-1, -1, -1};
    for (int i = 0; i < 3; ++i) {
        unlink_if_exists(dirs[i]);
        assert(mkdir(dirs[i], 0700) == 0);
        int subdirfd = openat(dirfd, dirs[i], O_RDONLY | O_DIRECTORY, 0);
        assert(subdirfd >= 0);
        fds[i] = openat(subdirfd, ".", O_TMPFILE | O_RDWR, 0600);
        assert(fds[i] >= 0);
        assert(close(subdirfd) == 0);
    }

    for (int i = 0; i < 3; ++i) {
        write_all(fds[i], "xyz", 3);
        assert(lseek(fds[i], 0, SEEK_SET) == 0);
        char buf[4] = {0};
        read_all(fds[i], buf, 3);
        assert(strcmp(buf, "xyz") == 0);
    }

    for (int i = 0; i < 3; ++i) {
        assert(close(fds[i]) == 0);
        unlink_if_exists(dirs[i]);
    }
#endif
}

static void test_o_tmpfile_perms(int dirfd)
{
    printf("Test 14: O_TMPFILE permissions respect umask\n");
#ifndef O_TMPFILE
    printf("  O_TMPFILE not defined, skipping\n");
#else
    const mode_t perms[] = {0777, 0644, 0440};
    mode_t old_mask = umask(0022);

    for (int i = 0; i < 3; ++i) {
        char name[32];
        snprintf(name, sizeof(name), "tmpfile_perm_%d", i);
        unlink_if_exists(name);

        int fd = openat(dirfd, ".", O_TMPFILE | O_RDWR, perms[i]);
        assert(fd >= 0);

        mode_t mask = 0022;
        link_tmpfile(dirfd, fd, name);

        struct stat st;
        assert(stat(name, &st) == 0);
        mode_t expected = perms[i] & ~mask;
        assert((st.st_mode & 07777) == expected);

        assert(close(fd) == 0);
        unlink_if_exists(name);
    }
    umask(old_mask);
#endif
}

int main(void)
{
    char cwd[PATH_MAX];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    const char *root = "path_open_test_root";
    ensure_clean_dir(root);
    assert(chdir(root) == 0);

    int dirfd = open(".", O_RDONLY | O_DIRECTORY);
    assert(dirfd >= 0);

    unlink_if_exists("abs_file");
    int abs_fd = openat(dirfd, "abs_file", O_CREAT | O_TRUNC | O_RDWR, 0600);
    assert(abs_fd >= 0);
    write_all(abs_fd, "abs", 3);
    assert(close(abs_fd) == 0);

    char abs_path[PATH_MAX];
    assert(getcwd(abs_path, sizeof(abs_path)) != NULL);
    strncat(abs_path, "/abs_file", sizeof(abs_path) - strlen(abs_path) - 1);

    test_relative_dirfd(dirfd);
    test_absolute_ignores_dirfd(dirfd, abs_path);
    test_dirfd_is_file();
    test_invalid_dirfd();
    test_at_fdcwd();
    test_o_append(dirfd);
    test_o_cloexec(dirfd);
    test_o_largefile(dirfd);
    test_o_noatime(dirfd);
    test_o_nofollow(dirfd);
    test_o_trunc(dirfd);
    test_o_tmpfile_basic(dirfd);
    test_o_tmpfile_multi_dirs(dirfd);
    test_o_tmpfile_perms(dirfd);

    unlink_if_exists("abs_file");
    assert(close(dirfd) == 0);
    assert(chdir(cwd) == 0);
    remove_tree(root);

    printf("All tests passed!\n");
    return 0;
}
