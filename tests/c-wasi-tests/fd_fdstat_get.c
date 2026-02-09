#include <stdio.h>
#include <stdlib.h>
#include <assert.h>
#include <stdint.h>

// WASI API headers
#include <wasi/api.h>

// Test 1: Basic fd_fdstat_get on standard file descriptors
void test_stdin_stdout_stderr() {
    printf("Test 1: fd_fdstat_get on stdin/stdout/stderr\n");

    __wasi_fdstat_t fdstat;
    __wasi_errno_t ret;

    // Test stdin
    ret = __wasi_fd_fdstat_get(0, &fdstat);
    assert(ret == 0 && "fd_fdstat_get(stdin) should succeed");
    assert(fdstat.fs_filetype == __WASI_FILETYPE_CHARACTER_DEVICE && "stdin should be CHARACTER_DEVICE");
    printf("  ✓ stdin: filetype=%d, flags=0x%x\n", fdstat.fs_filetype, fdstat.fs_flags);

    // Test stdout
    ret = __wasi_fd_fdstat_get(1, &fdstat);
    assert(ret == 0 && "fd_fdstat_get(stdout) should succeed");
    assert(fdstat.fs_filetype == __WASI_FILETYPE_CHARACTER_DEVICE && "stdout should be CHARACTER_DEVICE");
    printf("  ✓ stdout: filetype=%d, flags=0x%x\n", fdstat.fs_filetype, fdstat.fs_flags);

    // Test stderr
    ret = __wasi_fd_fdstat_get(2, &fdstat);
    assert(ret == 0 && "fd_fdstat_get(stderr) should succeed");
    assert(fdstat.fs_filetype == __WASI_FILETYPE_CHARACTER_DEVICE && "stderr should be CHARACTER_DEVICE");
    printf("  ✓ stderr: filetype=%d, flags=0x%x\n", fdstat.fs_filetype, fdstat.fs_flags);
}

// Test 2: fd_fdstat_get returns EBADF for invalid fd
void test_invalid_fd() {
    printf("\nTest 2: fd_fdstat_get with invalid fd (EBADF)\n");

    __wasi_fdstat_t fdstat;
    __wasi_errno_t ret;

    ret = __wasi_fd_fdstat_get(9999, &fdstat);
    assert(ret == __WASI_ERRNO_BADF && "fd_fdstat_get(9999) should return EBADF");
    printf("  ✓ Invalid fd 9999 returned EBADF (errno=%d)\n", ret);

    ret = __wasi_fd_fdstat_get(1500, &fdstat);
    assert(ret == __WASI_ERRNO_BADF && "fd_fdstat_get(1500) should return EBADF");
    printf("  ✓ Invalid fd 1500 returned EBADF (errno=%d)\n", ret);
}

// Test 3: fd_fdstat_get with invalid pointer
void test_invalid_pointer() {
    printf("\nTest 3: fd_fdstat_get with invalid pointer (MEMVIOLATION)\n");

    __wasi_errno_t ret = __wasi_fd_fdstat_get(0, (__wasi_fdstat_t *)0xFFFFFFFF);
    assert(ret == __WASI_ERRNO_MEMVIOLATION && "invalid pointer should return MEMVIOLATION");
    printf("  ✓ Invalid pointer returned MEMVIOLATION (errno=%d)\n", ret);
}

// Test 3: fd_fdstat_get consistency (multiple calls return same results)
void test_fdstat_consistency() {
    printf("\nTest 4: fd_fdstat_get consistency (repeated calls)\n");

    __wasi_fdstat_t fdstat1, fdstat2, fdstat3;
    __wasi_errno_t ret;

    ret = __wasi_fd_fdstat_get(0, &fdstat1);
    assert(ret == 0 && "fd_fdstat_get should succeed");

    ret = __wasi_fd_fdstat_get(0, &fdstat2);
    assert(ret == 0 && "fd_fdstat_get should succeed");

    ret = __wasi_fd_fdstat_get(0, &fdstat3);
    assert(ret == 0 && "fd_fdstat_get should succeed");

    assert(fdstat1.fs_filetype == fdstat2.fs_filetype && "filetypes should match");
    assert(fdstat1.fs_filetype == fdstat3.fs_filetype && "filetypes should match");
    assert(fdstat1.fs_flags == fdstat2.fs_flags && "flags should match");
    assert(fdstat1.fs_flags == fdstat3.fs_flags && "flags should match");
    assert(fdstat1.fs_rights_base == fdstat2.fs_rights_base && "rights_base should match");
    assert(fdstat1.fs_rights_base == fdstat3.fs_rights_base && "rights_base should match");

    printf("  ✓ All three calls returned consistent results\n");
}

// Test 4: All standard fds (stdin, stdout, stderr)
void test_all_standard_fds() {
    printf("\nTest 5: fd_fdstat_get on all standard fds\n");

    __wasi_fdstat_t fdstat;
    __wasi_errno_t ret;

    for (__wasi_fd_t fd = 0; fd <= 2; fd++) {
        ret = __wasi_fd_fdstat_get(fd, &fdstat);
        assert(ret == 0 && "fd_fdstat_get should succeed on standard fd");
        assert(fdstat.fs_filetype == __WASI_FILETYPE_CHARACTER_DEVICE && "should be CHARACTER_DEVICE");
        printf("  ✓ fd %d: filetype=%d\n", fd, fdstat.fs_filetype);
    }
}

// Test 5: fdstat structure fields validation
void test_fdstat_fields() {
    printf("\nTest 6: fdstat structure fields validation\n");

    __wasi_fdstat_t fdstat;
    __wasi_errno_t ret;

    ret = __wasi_fd_fdstat_get(0, &fdstat);
    assert(ret == 0 && "fd_fdstat_get should succeed");

    // Verify all fields are accessible
    printf("  fs_filetype: %d\n", fdstat.fs_filetype);
    printf("  fs_flags: 0x%x\n", fdstat.fs_flags);
    printf("  fs_rights_base: 0x%llx\n", (unsigned long long)fdstat.fs_rights_base);
    printf("  fs_rights_inheriting: 0x%llx\n", (unsigned long long)fdstat.fs_rights_inheriting);

    // Filetype should be valid
    assert(fdstat.fs_filetype >= 0 && fdstat.fs_filetype <= __WASI_FILETYPE_SOCKET_STREAM &&
           "filetype should be within valid range");

    printf("  ✓ All fields accessible and within valid ranges\n");
}

// Test 6: Preopen directory fd (if available)
void test_preopen_directory() {
    printf("\nTest 7: fd_fdstat_get on preopen directory\n");

    __wasi_fdstat_t fdstat;
    __wasi_errno_t ret;

    // fd 3 is usually the first preopen directory
    ret = __wasi_fd_fdstat_get(3, &fdstat);

    assert(ret == 0 && "fd_fdstat_get should succeed on preopen directory");
    assert(fdstat.fs_filetype == __WASI_FILETYPE_DIRECTORY && "preopen should be directory");
    printf("  ✓ Preopen directory: filetype=%d, rights_base=0x%llx\n",
           fdstat.fs_filetype, (unsigned long long)fdstat.fs_rights_base);
}

// Test 7: Rights validation - stdin should have read rights
void test_stdin_rights() {
    printf("\nTest 8: Rights validation - stdin should have read rights\n");

    __wasi_fdstat_t fdstat;
    __wasi_errno_t ret;

    ret = __wasi_fd_fdstat_get(0, &fdstat);
    assert(ret == 0 && "fd_fdstat_get should succeed");

    // stdin should have FD_READ right
    assert((fdstat.fs_rights_base & __WASI_RIGHTS_FD_READ) != 0 &&
           "stdin should have FD_READ right");
    printf("  ✓ stdin has FD_READ right: rights_base=0x%llx\n",
           (unsigned long long)fdstat.fs_rights_base);
}

// Test 8: Rights validation - stdout should have write rights
void test_stdout_rights() {
    printf("\nTest 9: Rights validation - stdout should have write rights\n");

    __wasi_fdstat_t fdstat;
    __wasi_errno_t ret;

    ret = __wasi_fd_fdstat_get(1, &fdstat);
    assert(ret == 0 && "fd_fdstat_get should succeed");

    // stdout should have FD_WRITE right
    assert((fdstat.fs_rights_base & __WASI_RIGHTS_FD_WRITE) != 0 &&
           "stdout should have FD_WRITE right");
    printf("  ✓ stdout has FD_WRITE right: rights_base=0x%llx\n",
           (unsigned long long)fdstat.fs_rights_base);
}

// Test 9: Rights validation - stderr should have write rights
void test_stderr_rights() {
    printf("\nTest 10: Rights validation - stderr should have write rights\n");

    __wasi_fdstat_t fdstat;
    __wasi_errno_t ret;

    ret = __wasi_fd_fdstat_get(2, &fdstat);
    assert(ret == 0 && "fd_fdstat_get should succeed");

    // stderr should have FD_WRITE right
    assert((fdstat.fs_rights_base & __WASI_RIGHTS_FD_WRITE) != 0 &&
           "stderr should have FD_WRITE right");
    printf("  ✓ stderr has FD_WRITE right: rights_base=0x%llx\n",
           (unsigned long long)fdstat.fs_rights_base);
}

// Test 10: File descriptor range testing
void test_fd_range() {
    printf("\nTest 11: File descriptor range testing\n");

    __wasi_fdstat_t fdstat;
    __wasi_errno_t ret;

    // Test a series of likely-invalid fds
    int invalid_fds[] = {100, 500, 1000, 5000, 10000, 65535};
    int invalid_count = sizeof(invalid_fds) / sizeof(invalid_fds[0]);

    for (int i = 0; i < invalid_count; i++) {
        ret = __wasi_fd_fdstat_get(invalid_fds[i], &fdstat);
        assert(ret == __WASI_ERRNO_BADF && "invalid fd should return EBADF");
    }

    printf("  ✓ All %d invalid fds returned EBADF\n", invalid_count);
}

// Test 11: Negative fd testing
void test_negative_fd() {
    printf("\nTest 12: Negative fd testing\n");

    __wasi_fdstat_t fdstat;
    __wasi_errno_t ret;

    // WASI fds are unsigned, so -1 becomes a large positive number
    // This should return EBADF
    ret = __wasi_fd_fdstat_get((__wasi_fd_t)-1, &fdstat);
    assert(ret == __WASI_ERRNO_BADF && "negative (wrapped) fd should return EBADF");
    printf("  ✓ Negative (wrapped) fd returned EBADF\n");
}

int main() {
    printf("WASIX fd_fdstat_get Integration Tests\n");
    printf("======================================\n\n");

    test_stdin_stdout_stderr();
    test_invalid_fd();
    test_invalid_pointer();
    test_fdstat_consistency();
    test_all_standard_fds();
    test_fdstat_fields();
    test_preopen_directory();
    test_stdin_rights();
    test_stdout_rights();
    test_stderr_rights();
    test_fd_range();
    test_negative_fd();

    printf("\n======================================\n");
    printf("✓ All fd_fdstat_get tests completed!\n");

    return 0;
}
