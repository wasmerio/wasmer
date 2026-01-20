#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

// WASIX API headers
#include <wasi/api_wasix.h>

static char base_path[PATH_MAX];
static size_t base_len = 0;

static __wasi_errno_t wasix_getcwd(char *buf, size_t buf_size, size_t *out_len) {
    __wasi_pointersize_t len = (__wasi_pointersize_t)buf_size;
    __wasi_errno_t ret = __wasi_getcwd((uint8_t *)buf, &len);
    if (out_len) {
        *out_len = (size_t)len;
    }
    return ret;
}

static void init_base_path(void) {
    char buf[PATH_MAX];
    size_t len = 0;
    __wasi_errno_t ret = wasix_getcwd(buf, sizeof(buf), &len);
    assert(ret == 0 && "__wasi_getcwd should succeed with large buffer");
    assert(len > 1 && len <= sizeof(buf));
    assert(buf[0] == '/' && "cwd should be absolute");

    buf[len - 1] = '\0';
    base_len = len - 1;
    memcpy(base_path, buf, base_len + 1);
}

// LTP getcwd02 + stress-ng stress-get: basic success and consistency
static void test_basic_getcwd(void) {
    printf("Test 1: __wasi_getcwd basic success\n");

    char buf[PATH_MAX];
    size_t len = 0;
    __wasi_errno_t ret = wasix_getcwd(buf, sizeof(buf), &len);
    assert(ret == 0 && "__wasi_getcwd should succeed");
    assert(len == base_len + 1 && "cwd length should include NUL");
    assert(memcmp(buf, base_path, base_len) == 0 && "cwd bytes should match base");
    assert(buf[base_len] == '\0' && "cwd should be NUL-terminated");

    char buf2[PATH_MAX];
    size_t len2 = 0;
    ret = wasix_getcwd(buf2, sizeof(buf2), &len2);
    assert(ret == 0 && "__wasi_getcwd should succeed (second call)");
    assert(len2 == base_len + 1 && "cwd length should include NUL");
    assert(memcmp(buf2, base_path, base_len) == 0 && "cwd bytes should match base");
    assert(buf2[base_len] == '\0' && "cwd should be NUL-terminated");

    printf("  OK getcwd returned consistent path: %s\n", base_path);
}

// LTP getcwd01: size errors (0, 1, too small) and length reporting
static void test_small_buffers(void) {
    printf("\nTest 2: __wasi_getcwd buffer too small errors\n");

    char buf[PATH_MAX];
    size_t len = 0;

    // Exact size should succeed.
    char *exact = (char *)malloc(base_len + 1);
    assert(exact != NULL && "malloc failed for exact buffer");
    __wasi_errno_t ret = wasix_getcwd(exact, base_len + 1, &len);
    assert(ret == 0 && "__wasi_getcwd should succeed with exact size");
    assert(len == base_len + 1 && "length should include NUL");
    assert(memcmp(exact, base_path, base_len) == 0 && "exact buffer content mismatch");
    assert(exact[base_len] == '\0' && "exact buffer should be NUL-terminated");
    free(exact);

    // Size 0 should report ERANGE and required length.
    len = 0;
    ret = wasix_getcwd(buf, 0, &len);
    assert(ret == __WASI_ERRNO_RANGE && "size 0 should return ERANGE");
    assert(len == base_len + 1 && "length should report required size on ERANGE");

    // Size 1: overflow unless cwd length is 1.
    len = 1;
    ret = wasix_getcwd(buf, 1, &len);
    if (base_len > 1) {
        assert(ret == __WASI_ERRNO_RANGE && "size 1 should return ERANGE");
        assert(len == base_len + 1 && "length should report required size on ERANGE");
    } else {
        assert(ret == 0 && "size 1 should succeed for cwd '/'");
        assert(len == base_len + 1 && "length should include NUL");
        assert(buf[0] == '/' && "cwd should be '/'");
        assert(buf[1] == '\0' && "cwd should be NUL-terminated");
    }

    // Size smaller than required should ERANGE.
    size_t small_len = base_len;
    len = small_len;
    ret = wasix_getcwd(buf, small_len, &len);
    assert(ret == __WASI_ERRNO_RANGE && "small buffer should return ERANGE");
    assert(len == base_len + 1 && "length should report required size on ERANGE");

    printf("  OK overflow cases reported required length %zu\n", base_len + 1);
}

// LTP getcwd01: bad address should fault
static void test_bad_address(void) {
    printf("\nTest 3: __wasi_getcwd bad address (EFAULT)\n");

    __wasi_pointersize_t len = (__wasi_pointersize_t)PATH_MAX;
    __wasi_errno_t ret = __wasi_getcwd((uint8_t *)0xFFFFFFFFu, &len);
    assert(ret == __WASI_ERRNO_FAULT && "bad address should return EFAULT");
    printf("  OK bad address returned EFAULT\n");
}

// LTP getcwd01: NULL buffer with insufficient size should ERANGE
static void test_null_buffer_overflow(void) {
    printf("\nTest 4: __wasi_getcwd NULL buffer ERANGE\n");

    __wasi_pointersize_t len = (__wasi_pointersize_t)(base_len > 1 ? base_len - 1 : 0);
    __wasi_errno_t ret = __wasi_getcwd(NULL, &len);
    assert(ret == __WASI_ERRNO_RANGE && "NULL buffer with small size should ERANGE");
    assert((size_t)len == base_len + 1 && "length should report required size on ERANGE");
    printf("  OK NULL buffer overflow reported required length %zu\n", base_len + 1);
}

// LTP getcwd02 + llvm libc getcwd tests (POSIX interface)
static void test_libc_getcwd(void) {
    printf("\nTest 5: libc getcwd behavior\n");

    char buf[PATH_MAX];
    errno = 0;
    char *res = getcwd(buf, sizeof(buf));
    assert(res != NULL && "getcwd should return the passed buffer");
    assert(res == buf && "getcwd should return the passed buffer");
    assert(strcmp(res, base_path) == 0 && "getcwd should match base path");

    errno = 0;
    res = getcwd(NULL, 0);
    assert(res != NULL && "getcwd(NULL, 0) should allocate and succeed");
    assert(strcmp(res, base_path) == 0 && "allocated getcwd should match base path");
    free(res);

    errno = 0;
    res = getcwd(NULL, PATH_MAX);
    assert(res != NULL && "getcwd(NULL, PATH_MAX) should allocate and succeed");
    assert(strcmp(res, base_path) == 0 && "allocated getcwd should match base path");
    free(res);

    errno = 0;
    res = getcwd(buf, 0);
    assert(res == NULL && "getcwd(buf, 0) should fail");
    assert(errno == EINVAL && "getcwd(buf, 0) should set errno=EINVAL");

    errno = 0;
    res = getcwd(buf, base_len);
    assert(res == NULL && "getcwd(buf, base_len) should fail (no space for NUL)");
    assert(errno == ERANGE && "getcwd(buf, base_len) should set errno=ERANGE");

    printf("  OK libc getcwd checks completed\n");
}

// stress-ng stress-get: repeated getcwd calls should be consistent
static void test_stress_getcwd(void) {
    printf("\nTest 6: libc getcwd stress loop\n");

    char buf[PATH_MAX];
    for (int i = 0; i < 1000; i++) {
        errno = 0;
        char *res = getcwd(buf, sizeof(buf));
        assert(res != NULL && "getcwd stress loop should succeed");
        assert(res == buf && "getcwd stress should return the passed buffer");
        assert(strcmp(res, base_path) == 0 && "getcwd stress should match base path");
    }

    printf("  OK getcwd stress loop completed\n");
}

// LTP getcwd03: symlink path should resolve to real path
static void test_symlink_resolution(void) {
    printf("\nTest 7: getcwd resolves symlink to real path\n");

    char dir[64];
    char link[64];
    char buf1[PATH_MAX];
    char buf2[PATH_MAX];
    char link_target[PATH_MAX];
    size_t len1 = 0;
    size_t len2 = 0;

    snprintf(dir, sizeof(dir), "getcwd_dir_%d", getpid());
    snprintf(link, sizeof(link), "getcwd_link_%d", getpid());

    if (mkdir(dir, 0755) != 0) {
        assert(0 && "mkdir(getcwd_dir) should succeed");
        return;
    }
    struct stat st;
    if (symlink(dir, link) != 0) {
        assert(0 && "symlink(getcwd_link) should succeed");
        goto cleanup;
    }
    if (lstat(link, &st) != 0) {
        assert(0 && "lstat(getcwd_link) should succeed");
        goto cleanup;
    }
    assert(S_ISLNK(st.st_mode) && "lstat(getcwd_link) should report symlink");

    if (chdir(dir) != 0) {
        assert(0 && "chdir(getcwd_dir) should succeed");
        goto cleanup;
    }
    __wasi_errno_t ret = wasix_getcwd(buf1, sizeof(buf1), &len1);
    if (ret != 0) {
        assert(0 && "__wasi_getcwd should succeed in real dir");
        goto cleanup;
    }

    if (chdir("..") != 0) {
        assert(0 && "chdir(..) should succeed");
        goto cleanup;
    }
    if (chdir(link) != 0) {
        assert(0 && "chdir(getcwd_link) should succeed");
        goto cleanup;
    }
    ret = wasix_getcwd(buf2, sizeof(buf2), &len2);
    if (ret != 0) {
        assert(0 && "__wasi_getcwd should succeed in symlink dir");
        goto cleanup;
    }

    assert(len1 == len2 && "cwd lengths should match");
    assert(memcmp(buf1, buf2, len1) == 0 && "cwd should resolve symlink to real path");

    if (chdir("..") != 0) {
        assert(0 && "chdir(..) after symlink should succeed");
        goto cleanup;
    }

    ssize_t link_len = readlink(link, link_target, sizeof(link_target));
    assert(link_len > 0 && "readlink should succeed");
    assert((size_t)link_len == strlen(dir) && "readlink length should match target");
    assert(memcmp(link_target, dir, (size_t)link_len) == 0 && "link target should match directory");

cleanup:
    if (chdir(base_path) != 0) {
        assert(0 && "chdir(base_path) should succeed");
    }
    if (unlink(link) != 0) {
        assert(0 && "unlink(getcwd_link) should succeed");
    }
    if (rmdir(dir) != 0) {
        assert(0 && "rmdir(getcwd_dir) should succeed");
    }

    printf("  OK symlink cwd resolved to real path\n");
}

// LTP getcwd04: rename race should not corrupt cwd
static atomic_int race_stop = 0;
static atomic_int race_error = 0;

static void *rename_thread(void *arg) {
    (void)arg;
    const char *a = "race_a";
    const char *b = "race_b";
    int toggle = 0;

    while (!atomic_load(&race_stop)) {
        const char *from = toggle ? b : a;
        const char *to = toggle ? a : b;
        if (rename(from, to) != 0) {
            atomic_store(&race_error, 1);
            break;
        }
        toggle = !toggle;
    }

    return NULL;
}

static void test_rename_race(void) {
    printf("\nTest 8: getcwd stable during rename race\n");

    char dir[64];
    const char *file_a = "race_a";
    int fd;
    pthread_t thread;

    snprintf(dir, sizeof(dir), "getcwd_race_%d", getpid());

    if (mkdir(dir, 0755) != 0) {
        assert(0 && "mkdir(getcwd_race) should succeed");
        return;
    }
    if (chdir(dir) != 0) {
        assert(0 && "chdir(getcwd_race) should succeed");
        goto cleanup;
    }

    char cwd_buf[PATH_MAX];
    size_t cwd_len = 0;
    __wasi_errno_t ret = wasix_getcwd(cwd_buf, sizeof(cwd_buf), &cwd_len);
    if (ret != 0) {
        assert(0 && "__wasi_getcwd should succeed in race dir");
        goto cleanup;
    }

    fd = open(file_a, O_CREAT | O_RDWR, 0644);
    if (fd < 0) {
        assert(0 && "open(race_a) should succeed");
        goto cleanup;
    }
    close(fd);

    atomic_store(&race_stop, 0);
    atomic_store(&race_error, 0);
    int rc = pthread_create(&thread, NULL, rename_thread, NULL);
    if (rc != 0) {
        assert(0 && "pthread_create should succeed");
        goto cleanup;
    }

    for (int i = 0; i < 1000; i++) {
        char check_buf[PATH_MAX];
        size_t check_len = 0;
        ret = wasix_getcwd(check_buf, sizeof(check_buf), &check_len);
        if (ret != 0) {
            assert(0 && "__wasi_getcwd should succeed during race");
            break;
        }
        assert(check_len == cwd_len && "cwd length should remain stable");
        assert(memcmp(check_buf, cwd_buf, cwd_len) == 0 &&
               "cwd should not change during rename race");
    }

    atomic_store(&race_stop, 1);
    pthread_join(thread, NULL);
    assert(atomic_load(&race_error) == 0 && "rename thread encountered an error");

cleanup:
    unlink("race_a");
    unlink("race_b");
    if (chdir(base_path) != 0) {
        assert(0 && "chdir(base_path) should succeed");
    }
    if (rmdir(dir) != 0) {
        assert(0 && "rmdir(getcwd_race) should succeed");
    }

    printf("  OK cwd stable during rename race\n");
}

int main(void) {
    printf("WASIX getcwd Integration Tests\n");
    printf("================================\n\n");

    init_base_path();

    test_basic_getcwd();
    test_small_buffers();
    test_bad_address();
    test_null_buffer_overflow();
    test_libc_getcwd();
    test_stress_getcwd();
    test_symlink_resolution();
    test_rename_race();

    printf("\n================================\n");
    printf("OK All getcwd tests completed!\n");

    return 0;
}
