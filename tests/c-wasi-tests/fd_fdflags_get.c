#include <stdio.h>
#include <stdlib.h>
#include <assert.h>
#include <stdint.h>
#include <fcntl.h>
#include <unistd.h>

// WASI API headers
#include <wasi/api.h>
#include <wasi/api_wasix.h>

// Test 1: Basic fd_fdflags_get on standard file descriptors
void test_stdin_stdout_stderr() {
    printf("Test 1: fd_fdflags_get on stdin/stdout/stderr\n");

    __wasi_fdflagsext_t flags;
    __wasi_errno_t ret;

    // Test stdin
    ret = __wasi_fd_fdflags_get(0, &flags);
    assert(ret == 0 && "fd_fdflags_get(stdin) should succeed");
    printf("  ✓ stdin: flags=0x%x\n", flags);

    // Test stdout
    ret = __wasi_fd_fdflags_get(1, &flags);
    assert(ret == 0 && "fd_fdflags_get(stdout) should succeed");
    printf("  ✓ stdout: flags=0x%x\n", flags);

    // Test stderr
    ret = __wasi_fd_fdflags_get(2, &flags);
    assert(ret == 0 && "fd_fdflags_get(stderr) should succeed");
    printf("  ✓ stderr: flags=0x%x\n", flags);
}

// Test 2: fd_fdflags_get returns EBADF for invalid fd
void test_invalid_fd() {
    printf("\nTest 2: fd_fdflags_get with invalid fd (EBADF)\n");

    __wasi_fdflagsext_t flags;
    __wasi_errno_t ret;

    ret = __wasi_fd_fdflags_get(9999, &flags);
    assert(ret == __WASI_ERRNO_BADF && "fd_fdflags_get(9999) should return EBADF");
    printf("  ✓ Invalid fd 9999 returned EBADF (errno=%d)\n", ret);

    ret = __wasi_fd_fdflags_get(1500, &flags);
    assert(ret == __WASI_ERRNO_BADF && "fd_fdflags_get(1500) should return EBADF");
    printf("  ✓ Invalid fd 1500 returned EBADF (errno=%d)\n", ret);
}

// Test 3: fd_fdflags_set returns EBADF for invalid fd
void test_set_invalid_fd() {
    printf("\nTest 3: fd_fdflags_set with invalid fd (EBADF)\n");

    __wasi_errno_t ret;

    ret = __wasi_fd_fdflags_set(9999, __WASI_FDFLAGSEXT_CLOEXEC);
    assert(ret == __WASI_ERRNO_BADF && "fd_fdflags_set(9999) should return EBADF");
    printf("  ✓ Invalid fd 9999 returned EBADF (errno=%d)\n", ret);

    ret = __wasi_fd_fdflags_set(1500, __WASI_FDFLAGSEXT_CLOEXEC);
    assert(ret == __WASI_ERRNO_BADF && "fd_fdflags_set(1500) should return EBADF");
    printf("  ✓ Invalid fd 1500 returned EBADF (errno=%d)\n", ret);
}

// Test 4: fd_fdflags_get consistency (multiple calls return same results)
void test_fdflags_consistency() {
    printf("\nTest 4: fd_fdflags_get consistency (repeated calls)\n");

    __wasi_fdflagsext_t flags1, flags2, flags3;
    __wasi_errno_t ret;

    ret = __wasi_fd_fdflags_get(0, &flags1);
    assert(ret == 0 && "fd_fdflags_get should succeed");

    ret = __wasi_fd_fdflags_get(0, &flags2);
    assert(ret == 0 && "fd_fdflags_get should succeed");

    ret = __wasi_fd_fdflags_get(0, &flags3);
    assert(ret == 0 && "fd_fdflags_get should succeed");

    assert(flags1 == flags2 && "flags should match");
    assert(flags1 == flags3 && "flags should match");

    printf("  ✓ All three calls returned consistent results (flags=0x%x)\n", flags1);
}

// Test 5: All standard fds (stdin, stdout, stderr)
void test_all_standard_fds() {
    printf("\nTest 5: fd_fdflags_get on all standard fds\n");

    __wasi_fdflagsext_t flags;
    __wasi_errno_t ret;

    for (__wasi_fd_t fd = 0; fd <= 2; fd++) {
        ret = __wasi_fd_fdflags_get(fd, &flags);
        assert(ret == 0 && "fd_fdflags_get should succeed on standard fd");
        printf("  ✓ fd %d: flags=0x%x\n", fd, flags);
    }
}

// Test 6: CLOEXEC flag setting and getting
void test_cloexec_flag() {
    printf("\nTest 6: CLOEXEC flag set/get operations\n");

    __wasi_fdflagsext_t flags;
    __wasi_errno_t ret;

    int file_fd = open("test_cloexec_file.txt", O_CREAT | O_RDWR, 0644);
    assert(file_fd >= 0 && "open(test_cloexec_file.txt) should succeed");

    // Check initial flags (should be 0)
    ret = __wasi_fd_fdflags_get(file_fd, &flags);
    assert(ret == 0 && "fd_fdflags_get should succeed");
    printf("  ✓ Initial flags: 0x%x\n", flags);

    // Set CLOEXEC flag
    ret = __wasi_fd_fdflags_set(file_fd, __WASI_FDFLAGSEXT_CLOEXEC);
    assert(ret == 0 && "fd_fdflags_set should succeed");
    printf("  ✓ Set CLOEXEC flag\n");

    // Verify flag is set
    ret = __wasi_fd_fdflags_get(file_fd, &flags);
    assert(ret == 0 && "fd_fdflags_get should succeed");
    assert((flags & __WASI_FDFLAGSEXT_CLOEXEC) != 0 && "CLOEXEC flag should be set");
    printf("  ✓ CLOEXEC flag verified: 0x%x\n", flags);

    // Clear CLOEXEC flag
    ret = __wasi_fd_fdflags_set(file_fd, 0);
    assert(ret == 0 && "fd_fdflags_set should succeed");
    printf("  ✓ Cleared CLOEXEC flag\n");

    // Verify flag is cleared
    ret = __wasi_fd_fdflags_get(file_fd, &flags);
    assert(ret == 0 && "fd_fdflags_get should succeed");
    assert((flags & __WASI_FDFLAGSEXT_CLOEXEC) == 0 && "CLOEXEC flag should be cleared");
    printf("  ✓ CLOEXEC flag cleared: 0x%x\n", flags);

    // Close and clean up
    close(file_fd);
    unlink("test_cloexec_file.txt");
}

// Test 7: fd_fdflags_get after fd close (should fail)
void test_flags_after_close() {
    printf("\nTest 7: fd_fdflags_get after fd close (EBADF)\n");

    __wasi_fdflagsext_t flags;
    __wasi_errno_t ret;

    int file_fd = open("test_close_file.txt", O_CREAT | O_RDWR, 0644);
    assert(file_fd >= 0 && "open(test_close_file.txt) should succeed");

    // Verify we can get flags before close
    ret = __wasi_fd_fdflags_get(file_fd, &flags);
    assert(ret == 0 && "fd_fdflags_get should succeed before close");
    printf("  ✓ Flags before close: 0x%x\n", flags);

    // Close the fd
    close(file_fd);
    printf("  ✓ Closed fd %d\n", file_fd);

    // Try to get flags after close (should fail)
    ret = __wasi_fd_fdflags_get(file_fd, &flags);
    assert(ret == __WASI_ERRNO_BADF && "fd_fdflags_get should return EBADF after close");
    printf("  ✓ fd_fdflags_get after close returned EBADF (errno=%d)\n", ret);

    unlink("test_close_file.txt");
}

// Test 8: fd_fdflags_set on closed fd (should fail)
void test_set_flags_on_closed_fd() {
    printf("\nTest 8: fd_fdflags_set on closed fd (EBADF)\n");

    __wasi_errno_t ret;

    int file_fd = open("test_set_close_file.txt", O_CREAT | O_RDWR, 0644);
    assert(file_fd >= 0 && "open(test_set_close_file.txt) should succeed");

    // Close the fd
    close(file_fd);
    printf("  ✓ Closed fd %d\n", file_fd);

    // Try to set flags after close (should fail)
    ret = __wasi_fd_fdflags_set(file_fd, __WASI_FDFLAGSEXT_CLOEXEC);
    assert(ret == __WASI_ERRNO_BADF && "fd_fdflags_set should return EBADF on closed fd");
    printf("  ✓ fd_fdflags_set on closed fd returned EBADF (errno=%d)\n", ret);

    unlink("test_set_close_file.txt");
}

// Test 9: File descriptor range testing
void test_fd_range() {
    printf("\nTest 9: File descriptor range testing\n");

    __wasi_fdflagsext_t flags;
    __wasi_errno_t ret;

    // Test a series of likely-invalid fds
    int invalid_fds[] = {100, 500, 1000, 5000, 10000, 65535};
    int invalid_count = sizeof(invalid_fds) / sizeof(invalid_fds[0]);

    for (int i = 0; i < invalid_count; i++) {
        ret = __wasi_fd_fdflags_get(invalid_fds[i], &flags);
        assert(ret == __WASI_ERRNO_BADF && "invalid fd should return EBADF");
    }

    printf("  ✓ All %d invalid fds returned EBADF\n", invalid_count);
}

// Test 10: Negative fd testing
void test_negative_fd() {
    printf("\nTest 10: Negative fd testing\n");

    __wasi_fdflagsext_t flags;
    __wasi_errno_t ret;

    // WASI fds are unsigned, so -1 becomes a large positive number
    // This should return EBADF
    ret = __wasi_fd_fdflags_get((__wasi_fd_t)-1, &flags);
    assert(ret == __WASI_ERRNO_BADF && "negative (wrapped) fd should return EBADF");
    printf("  ✓ Negative (wrapped) fd returned EBADF\n");
}

// Test 11: Preopen directory flags
void test_preopen_directory() {
    printf("\nTest 11: fd_fdflags_get on preopen directory\n");

    __wasi_fdflagsext_t flags;
    __wasi_errno_t ret;

    // fd 3 is usually the first preopen directory
    ret = __wasi_fd_fdflags_get(3, &flags);

    assert(ret == 0 && "fd_fdflags_get should succeed on preopen directory");
    printf("  ✓ Preopen directory flags: 0x%x\n", flags);
}

// Test 12: fd_fdflags_get vs fd_fdstat_get comparison
void test_fdflags_vs_fdstat() {
    printf("\nTest 12: fd_fdflags_get vs fd_fdstat_get comparison\n");

    __wasi_fdflagsext_t fdflags;
    __wasi_fdstat_t fdstat;
    __wasi_errno_t ret;

    // Get both fd_flags and fdstat for stdin
    ret = __wasi_fd_fdflags_get(0, &fdflags);
    assert(ret == 0 && "fd_fdflags_get should succeed");

    ret = __wasi_fd_fdstat_get(0, &fdstat);
    assert(ret == 0 && "fd_fdstat_get should succeed");

    printf("  fd_fdflags_get returned: 0x%x\n", fdflags);
    printf("  fd_fdstat_get fs_flags: 0x%x\n", fdstat.fs_flags);
    printf("  ✓ Both syscalls succeeded (values may differ - different flag types)\n");
}

int main() {
    printf("WASIX fd_fdflags_get Integration Tests\n");
    printf("=======================================\n\n");

    test_stdin_stdout_stderr();
    test_invalid_fd();
    test_set_invalid_fd();
    test_fdflags_consistency();
    test_all_standard_fds();
    test_cloexec_flag();
    test_flags_after_close();
    test_set_flags_on_closed_fd();
    test_fd_range();
    test_negative_fd();
    test_preopen_directory();
    test_fdflags_vs_fdstat();

    printf("\n=======================================\n");
    printf("✓ All fd_fdflags_get tests completed!\n");

    return 0;
}
