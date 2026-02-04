#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include <wasi/api_wasi.h>

struct entry {
    char name[256];
    __wasi_dircookie_t next;
    __wasi_filetype_t type;
    __wasi_inode_t ino;
    size_t namelen;
};

static void make_unique_name(char *out, size_t cap, const char *prefix)
{
    uint32_t rand = 0;
    __wasi_errno_t err = __wasi_random_get((uint8_t *)&rand, sizeof(rand));
    if (err != __WASI_ERRNO_SUCCESS) {
        rand = 0xdeadbeef;
    }
    snprintf(out, cap, "%s_%08x", prefix, rand);
}

static __wasi_errno_t collect_entries(int fd,
                                      __wasi_dircookie_t cookie,
                                      size_t buf_len,
                                      struct entry *out,
                                      size_t out_cap,
                                      size_t *out_count,
                                      size_t *out_used)
{
    uint8_t *buf = malloc(buf_len);
    assert(buf != NULL);
    __wasi_size_t used = 0;
    __wasi_errno_t err = __wasi_fd_readdir((__wasi_fd_t)fd, buf, buf_len, cookie, &used);
    if (err != __WASI_ERRNO_SUCCESS) {
        free(buf);
        *out_count = 0;
        *out_used = 0;
        return err;
    }

    size_t count = 0;
    size_t offset = 0;
    while (offset + sizeof(__wasi_dirent_t) <= used) {
        const __wasi_dirent_t *dirent = (const __wasi_dirent_t *)(buf + offset);
        size_t name_len = dirent->d_namlen;
        size_t entry_size = sizeof(__wasi_dirent_t) + name_len;
        if (offset + entry_size > used) {
            break;
        }
        if (count < out_cap) {
            struct entry *e = &out[count];
            size_t copy_len = name_len < (sizeof(e->name) - 1) ? name_len : (sizeof(e->name) - 1);
            memcpy(e->name, buf + offset + sizeof(__wasi_dirent_t), copy_len);
            e->name[copy_len] = '\0';
            e->namelen = name_len;
            e->next = dirent->d_next;
            e->type = dirent->d_type;
            e->ino = dirent->d_ino;
            count++;
        }
        offset += entry_size;
    }

    free(buf);
    *out_count = count;
    *out_used = used;
    return __WASI_ERRNO_SUCCESS;
}

static int find_entry(const struct entry *entries, size_t count, const char *name)
{
    for (size_t i = 0; i < count; i++) {
        if (strcmp(entries[i].name, name) == 0) {
            return (int)i;
        }
    }
    return -1;
}

static void expect_errno(__wasi_errno_t err, __wasi_errno_t expected, const char *msg)
{
    if (err != expected) {
        fprintf(stderr, "FAIL: %s (got %u expected %u)\n",
                msg, (unsigned)err, (unsigned)expected);
        assert(err == expected);
    }
}

static void test_empty_dir_basic(void)
{
    printf("Test 1: empty dir includes . and .. with correct types\n");
    char dirname[64];
    make_unique_name(dirname, sizeof(dirname), "fd_readdir_empty");
    assert(mkdir(dirname, 0700) == 0);

    int fd = open(dirname, O_RDONLY | O_DIRECTORY);
    assert(fd >= 0);

    struct entry entries[8];
    size_t count = 0;
    size_t used = 0;
    __wasi_errno_t err = collect_entries(fd, 0, 256, entries, 8, &count, &used);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(used <= 256);

    int dot = find_entry(entries, count, ".");
    int dotdot = find_entry(entries, count, "..");
    assert(dot >= 0);
    assert(dotdot >= 0);
    assert(entries[dot].type == __WASI_FILETYPE_DIRECTORY);
    assert(entries[dotdot].type == __WASI_FILETYPE_DIRECTORY);
    assert(entries[dot].namelen == 1);
    assert(entries[dotdot].namelen == 2);

    close(fd);
    assert(rmdir(dirname) == 0);
}

static void test_entries_and_types(void)
{
    printf("Test 2: directory entries include file/dir/symlink with types\n");
    char dirname[64];
    char file[96];
    char subdir[96];
    char symlink_path[96];
    make_unique_name(dirname, sizeof(dirname), "fd_readdir_entries");
    snprintf(file, sizeof(file), "%s/file", dirname);
    snprintf(subdir, sizeof(subdir), "%s/nested", dirname);
    snprintf(symlink_path, sizeof(symlink_path), "%s/symlink", dirname);

    assert(mkdir(dirname, 0700) == 0);
    int fd_file = open(file, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd_file >= 0);
    close(fd_file);
    assert(mkdir(subdir, 0700) == 0);
    assert(symlink("target_missing", symlink_path) == 0);

    int dirfd = open(dirname, O_RDONLY | O_DIRECTORY);
    assert(dirfd >= 0);

    struct entry entries[16];
    size_t count = 0;
    size_t used = 0;
    __wasi_errno_t err = collect_entries(dirfd, 0, 512, entries, 16, &count, &used);
    assert(err == __WASI_ERRNO_SUCCESS);

    int i_file = find_entry(entries, count, "file");
    int i_dir = find_entry(entries, count, "nested");
    int i_link = find_entry(entries, count, "symlink");
    assert(i_file >= 0);
    assert(i_dir >= 0);
    assert(i_link >= 0);

    assert(entries[i_file].type == __WASI_FILETYPE_REGULAR_FILE);
    assert(entries[i_dir].type == __WASI_FILETYPE_DIRECTORY);
    assert(entries[i_link].type == __WASI_FILETYPE_SYMBOLIC_LINK);

    __wasi_filestat_t st_file;
    __wasi_filestat_t st_dir;
    int fd_check_file = open(file, O_RDONLY);
    assert(fd_check_file >= 0);
    err = __wasi_fd_filestat_get((__wasi_fd_t)fd_check_file, &st_file);
    assert(err == __WASI_ERRNO_SUCCESS);
    close(fd_check_file);

    int fd_check_dir = open(subdir, O_RDONLY | O_DIRECTORY);
    assert(fd_check_dir >= 0);
    err = __wasi_fd_filestat_get((__wasi_fd_t)fd_check_dir, &st_dir);
    assert(err == __WASI_ERRNO_SUCCESS);
    close(fd_check_dir);

    assert(entries[i_file].ino == st_file.ino);
    assert(entries[i_dir].ino == st_dir.ino);

    close(dirfd);
    assert(unlink(symlink_path) == 0);
    assert(rmdir(subdir) == 0);
    assert(unlink(file) == 0);
    assert(rmdir(dirname) == 0);
}

static void test_cookie_and_past_end(void)
{
    printf("Test 3: cookies advance and past-end returns 0\n");
    char dirname[64];
    char file_a[96];
    char file_b[96];
    make_unique_name(dirname, sizeof(dirname), "fd_readdir_cookie");
    snprintf(file_a, sizeof(file_a), "%s/a", dirname);
    snprintf(file_b, sizeof(file_b), "%s/b", dirname);
    assert(mkdir(dirname, 0700) == 0);
    assert(close(open(file_a, O_CREAT | O_TRUNC | O_RDWR, 0644)) == 0);
    assert(close(open(file_b, O_CREAT | O_TRUNC | O_RDWR, 0644)) == 0);

    int dirfd = open(dirname, O_RDONLY | O_DIRECTORY);
    assert(dirfd >= 0);

    struct entry entries[16];
    size_t count = 0;
    size_t used = 0;
    __wasi_errno_t err = collect_entries(dirfd, 0, 512, entries, 16, &count, &used);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(count >= 2);

    const char *first_name = entries[0].name;
    __wasi_dircookie_t cookie = entries[0].next;

    struct entry entries2[16];
    size_t count2 = 0;
    size_t used2 = 0;
    err = collect_entries(dirfd, cookie, 512, entries2, 16, &count2, &used2);
    assert(err == __WASI_ERRNO_SUCCESS);
    for (size_t i = 0; i < count2; i++) {
        assert(strcmp(entries2[i].name, first_name) != 0);
    }

    __wasi_dircookie_t max_cookie = 0;
    for (size_t i = 0; i < count; i++) {
        if (entries[i].next > max_cookie) {
            max_cookie = entries[i].next;
        }
    }
    size_t count3 = 0;
    size_t used3 = 0;
    err = collect_entries(dirfd, max_cookie + 1, 256, entries2, 16, &count3, &used3);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(used3 == 0);
    assert(count3 == 0);

    close(dirfd);
    assert(unlink(file_a) == 0);
    assert(unlink(file_b) == 0);
    assert(rmdir(dirname) == 0);
}

static void test_large_dir_multiple_reads(void)
{
    printf("Test 4: large directory requires multiple reads\n");
    char dirname[64];
    make_unique_name(dirname, sizeof(dirname), "fd_readdir_large");
    assert(mkdir(dirname, 0700) == 0);
    for (int i = 0; i < 200; i++) {
        char path[96];
        snprintf(path, sizeof(path), "%s/file_%d", dirname, i);
        int fd = open(path, O_CREAT | O_TRUNC | O_RDWR, 0644);
        assert(fd >= 0);
        close(fd);
    }

    int dirfd = open(dirname, O_RDONLY | O_DIRECTORY);
    assert(dirfd >= 0);

    size_t total = 0;
    __wasi_dircookie_t cookie = 0;
    for (;;) {
        struct entry entries[32];
        size_t count = 0;
        size_t used = 0;
        __wasi_errno_t err = collect_entries(dirfd, cookie, 128, entries, 32, &count, &used);
        assert(err == __WASI_ERRNO_SUCCESS);
        if (count == 0) {
            break;
        }
        total += count;
        cookie = entries[count - 1].next;
        if (used < 128) {
            break;
        }
    }
    assert(total == 202);

    close(dirfd);
    for (int i = 0; i < 200; i++) {
        char path[96];
        snprintf(path, sizeof(path), "%s/file_%d", dirname, i);
        assert(unlink(path) == 0);
    }
    assert(rmdir(dirname) == 0);
}

static void test_unicode_name(void)
{
    printf("Test 5: unicode filename is returned intact\n");
    char dirname[64];
    char filename[128];
    make_unique_name(dirname, sizeof(dirname), "fd_readdir_unicode");
    snprintf(filename, sizeof(filename), "%s/Действие", dirname);
    assert(mkdir(dirname, 0700) == 0);
    int fd = open(filename, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    close(fd);

    int dirfd = open(dirname, O_RDONLY | O_DIRECTORY);
    assert(dirfd >= 0);

    struct entry entries[16];
    size_t count = 0;
    size_t used = 0;
    __wasi_errno_t err = collect_entries(dirfd, 0, 512, entries, 16, &count, &used);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(find_entry(entries, count, "Действие") >= 0);

    close(dirfd);
    assert(unlink(filename) == 0);
    assert(rmdir(dirname) == 0);
}

static void test_error_cases(void)
{
    printf("Test 6: error cases (EBADF, ENOTDIR, ENOENT, EINVAL, MEMVIOLATION)\n");
    __wasi_size_t used = 0;

    __wasi_errno_t err = __wasi_fd_readdir((__wasi_fd_t)-1, (uint8_t *)0x1000, 64, 0, &used);
    expect_errno(err, __WASI_ERRNO_BADF, "invalid fd should be BADF");

    char file_path[64];
    make_unique_name(file_path, sizeof(file_path), "fd_readdir_notdir");
    int fd_file = open(file_path, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd_file >= 0);
    err = __wasi_fd_readdir((__wasi_fd_t)fd_file, (uint8_t *)0x1000, 64, 0, &used);
    expect_errno(err, __WASI_ERRNO_NOTDIR, "file fd should be NOTDIR");
    close(fd_file);
    assert(unlink(file_path) == 0);

    char deleted_dir[64];
    make_unique_name(deleted_dir, sizeof(deleted_dir), "fd_readdir_deleted");
    assert(mkdir(deleted_dir, 0700) == 0);
    int fd_dir = open(deleted_dir, O_RDONLY | O_DIRECTORY);
    assert(fd_dir >= 0);
    assert(rmdir(deleted_dir) == 0);
    err = __wasi_fd_readdir((__wasi_fd_t)fd_dir, (uint8_t *)0x1000, 64, 0, &used);
    expect_errno(err, __WASI_ERRNO_NOENT, "deleted directory should be NOENT");
    close(fd_dir);

    char small_dir[64];
    make_unique_name(small_dir, sizeof(small_dir), "fd_readdir_smallbuf");
    assert(mkdir(small_dir, 0700) == 0);
    int fd_small = open(small_dir, O_RDONLY | O_DIRECTORY);
    assert(fd_small >= 0);
    err = __wasi_fd_readdir((__wasi_fd_t)fd_small, (uint8_t *)0x1000, 1, 0, &used);
    expect_errno(err, __WASI_ERRNO_INVAL, "buffer too small should be INVAL");
    close(fd_small);
    assert(rmdir(small_dir) == 0);

    char badptr_dir[64];
    make_unique_name(badptr_dir, sizeof(badptr_dir), "fd_readdir_badptr");
    assert(mkdir(badptr_dir, 0700) == 0);
    int fd_bad = open(badptr_dir, O_RDONLY | O_DIRECTORY);
    assert(fd_bad >= 0);
    err = __wasi_fd_readdir((__wasi_fd_t)fd_bad, (uint8_t *)0xFFFFFFFF, 64, 0, &used);
    expect_errno(err, __WASI_ERRNO_MEMVIOLATION, "invalid buffer pointer should be MEMVIOLATION");
    close(fd_bad);
    assert(rmdir(badptr_dir) == 0);
}

static void test_dot_inode_matches_filestat(void)
{
    printf("Test 7: dot inode matches fd_filestat_get\n");
    char dirname[64];
    make_unique_name(dirname, sizeof(dirname), "fd_readdir_inode");
    assert(mkdir(dirname, 0700) == 0);

    int fd = open(dirname, O_RDONLY | O_DIRECTORY);
    assert(fd >= 0);

    struct entry entries[8];
    size_t count = 0;
    size_t used = 0;
    __wasi_errno_t err = collect_entries(fd, 0, 256, entries, 8, &count, &used);
    assert(err == __WASI_ERRNO_SUCCESS);

    int dot = find_entry(entries, count, ".");
    assert(dot >= 0);

    __wasi_filestat_t stat;
    err = __wasi_fd_filestat_get((__wasi_fd_t)fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(entries[dot].ino == stat.ino);

    close(fd);
    assert(rmdir(dirname) == 0);
}

int main(void)
{
    test_empty_dir_basic();
    test_cookie_and_past_end();
    test_large_dir_multiple_reads();
    test_unicode_name();
    test_error_cases();
    test_entries_and_types();
    test_dot_inode_matches_filestat();
    printf("All tests passed!\n");
    return 0;
}
