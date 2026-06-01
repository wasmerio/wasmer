#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <wasi/api.h>
#include <wasi/api_wasix.h>

// Scan a small fd window for the first preopened directory exposed by the
// current harness instead of assuming a fixed descriptor number.
#define PREOPEN_SCAN_LIMIT 32

static __wasi_fd_t find_preopen_directory_fd(void) {
  for (__wasi_fd_t fd = 3; fd < PREOPEN_SCAN_LIMIT; fd++) {
    __wasi_prestat_t prestat;
    __wasi_errno_t ret = __wasi_fd_prestat_get(fd, &prestat);
    if (ret == __WASI_ERRNO_SUCCESS && prestat.tag == __WASI_PREOPENTYPE_DIR) {
      return fd;
    }
  }

  assert(!"failed to find a preopened directory fd");
  return 0;
}

static void test_stdin_stdout_stderr(void) {
  printf("Test 1: fd_fdflags_get on stdin/stdout/stderr\n");

  __wasi_fdflagsext_t flags;
  __wasi_errno_t ret;

  ret = __wasi_fd_fdflags_get(0, &flags);
  assert(ret == 0 && "fd_fdflags_get(stdin) should succeed");
  printf("  stdin: flags=0x%x\n", flags);

  ret = __wasi_fd_fdflags_get(1, &flags);
  assert(ret == 0 && "fd_fdflags_get(stdout) should succeed");
  printf("  stdout: flags=0x%x\n", flags);

  ret = __wasi_fd_fdflags_get(2, &flags);
  assert(ret == 0 && "fd_fdflags_get(stderr) should succeed");
  printf("  stderr: flags=0x%x\n", flags);
}

static void test_invalid_fd(void) {
  printf("\nTest 2: fd_fdflags_get with invalid fd (EBADF)\n");

  __wasi_fdflagsext_t flags;
  __wasi_errno_t ret;

  ret = __wasi_fd_fdflags_get(9999, &flags);
  assert(ret == __WASI_ERRNO_BADF &&
         "fd_fdflags_get(9999) should return EBADF");
  printf("  Invalid fd 9999 returned EBADF (errno=%d)\n", ret);

  ret = __wasi_fd_fdflags_get(1500, &flags);
  assert(ret == __WASI_ERRNO_BADF &&
         "fd_fdflags_get(1500) should return EBADF");
  printf("  Invalid fd 1500 returned EBADF (errno=%d)\n", ret);
}

static void test_set_invalid_fd(void) {
  printf("\nTest 3: fd_fdflags_set with invalid fd (EBADF)\n");

  __wasi_errno_t ret;

  ret = __wasi_fd_fdflags_set(9999, __WASI_FDFLAGSEXT_CLOEXEC);
  assert(ret == __WASI_ERRNO_BADF &&
         "fd_fdflags_set(9999) should return EBADF");
  printf("  Invalid fd 9999 returned EBADF (errno=%d)\n", ret);

  ret = __wasi_fd_fdflags_set(1500, __WASI_FDFLAGSEXT_CLOEXEC);
  assert(ret == __WASI_ERRNO_BADF &&
         "fd_fdflags_set(1500) should return EBADF");
  printf("  Invalid fd 1500 returned EBADF (errno=%d)\n", ret);
}

static void test_fdflags_consistency(void) {
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

  printf("  All three calls returned consistent results (flags=0x%x)\n",
         flags1);
}

static void test_all_standard_fds(void) {
  printf("\nTest 5: fd_fdflags_get on all standard fds\n");

  __wasi_fdflagsext_t flags;
  __wasi_errno_t ret;

  for (__wasi_fd_t fd = 0; fd <= 2; fd++) {
    ret = __wasi_fd_fdflags_get(fd, &flags);
    assert(ret == 0 && "fd_fdflags_get should succeed on standard fd");
    printf("  fd %d: flags=0x%x\n", fd, flags);
  }
}

static void test_cloexec_flag(void) {
  printf("\nTest 6: CLOEXEC flag set/get operations\n");

  __wasi_fdflagsext_t flags;
  __wasi_errno_t ret;

  int file_fd = open("test_cloexec_file.txt", O_CREAT | O_RDWR, 0644);
  assert(file_fd >= 0 && "open(test_cloexec_file.txt) should succeed");

  ret = __wasi_fd_fdflags_get(file_fd, &flags);
  assert(ret == 0 && "fd_fdflags_get should succeed");
  printf("  Initial flags: 0x%x\n", flags);

  ret = __wasi_fd_fdflags_set(file_fd, __WASI_FDFLAGSEXT_CLOEXEC);
  assert(ret == 0 && "fd_fdflags_set should succeed");
  printf("  Set CLOEXEC flag\n");

  ret = __wasi_fd_fdflags_get(file_fd, &flags);
  assert(ret == 0 && "fd_fdflags_get should succeed");
  assert((flags & __WASI_FDFLAGSEXT_CLOEXEC) != 0 &&
         "CLOEXEC flag should be set");
  printf("  CLOEXEC flag verified: 0x%x\n", flags);

  ret = __wasi_fd_fdflags_set(file_fd, 0);
  assert(ret == 0 && "fd_fdflags_set should succeed");
  printf("  Cleared CLOEXEC flag\n");

  ret = __wasi_fd_fdflags_get(file_fd, &flags);
  assert(ret == 0 && "fd_fdflags_get should succeed");
  assert((flags & __WASI_FDFLAGSEXT_CLOEXEC) == 0 &&
         "CLOEXEC flag should be cleared");
  printf("  CLOEXEC flag cleared: 0x%x\n", flags);

  close(file_fd);
  unlink("test_cloexec_file.txt");
}

static void test_flags_after_close(void) {
  printf("\nTest 7: fd_fdflags_get after fd close (EBADF)\n");

  __wasi_fdflagsext_t flags;
  __wasi_errno_t ret;

  int file_fd = open("test_close_file.txt", O_CREAT | O_RDWR, 0644);
  assert(file_fd >= 0 && "open(test_close_file.txt) should succeed");

  ret = __wasi_fd_fdflags_get(file_fd, &flags);
  assert(ret == 0 && "fd_fdflags_get should succeed before close");
  printf("  Flags before close: 0x%x\n", flags);

  close(file_fd);
  printf("  Closed fd %d\n", file_fd);

  ret = __wasi_fd_fdflags_get(file_fd, &flags);
  assert(ret == __WASI_ERRNO_BADF &&
         "fd_fdflags_get should return EBADF after close");
  printf("  fd_fdflags_get after close returned EBADF (errno=%d)\n", ret);

  unlink("test_close_file.txt");
}

static void test_set_flags_on_closed_fd(void) {
  printf("\nTest 8: fd_fdflags_set on closed fd (EBADF)\n");

  __wasi_errno_t ret;

  int file_fd = open("test_set_close_file.txt", O_CREAT | O_RDWR, 0644);
  assert(file_fd >= 0 && "open(test_set_close_file.txt) should succeed");

  close(file_fd);
  printf("  Closed fd %d\n", file_fd);

  ret = __wasi_fd_fdflags_set(file_fd, __WASI_FDFLAGSEXT_CLOEXEC);
  assert(ret == __WASI_ERRNO_BADF &&
         "fd_fdflags_set should return EBADF on closed fd");
  printf("  fd_fdflags_set on closed fd returned EBADF (errno=%d)\n", ret);

  unlink("test_set_close_file.txt");
}

static void test_fd_range(void) {
  printf("\nTest 9: File descriptor range testing\n");

  __wasi_fdflagsext_t flags;
  __wasi_errno_t ret;

  int invalid_fds[] = {100, 500, 1000, 5000, 10000, 65535};
  int invalid_count = sizeof(invalid_fds) / sizeof(invalid_fds[0]);

  for (int i = 0; i < invalid_count; i++) {
    ret = __wasi_fd_fdflags_get(invalid_fds[i], &flags);
    assert(ret == __WASI_ERRNO_BADF && "invalid fd should return EBADF");
  }

  printf("  All %d invalid fds returned EBADF\n", invalid_count);
}

static void test_negative_fd(void) {
  printf("\nTest 10: Negative fd testing\n");

  __wasi_fdflagsext_t flags;
  __wasi_errno_t ret;

  ret = __wasi_fd_fdflags_get((__wasi_fd_t)-1, &flags);
  assert(ret == __WASI_ERRNO_BADF &&
         "negative (wrapped) fd should return EBADF");
  printf("  Negative (wrapped) fd returned EBADF\n");
}

static void test_preopen_directory(void) {
  printf("\nTest 11: fd_fdflags_get on preopen directory\n");

  __wasi_fdflagsext_t flags;
  __wasi_fd_t preopen_fd = find_preopen_directory_fd();
  __wasi_errno_t ret = __wasi_fd_fdflags_get(preopen_fd, &flags);

  assert(ret == 0 && "fd_fdflags_get should succeed on preopen directory");
  printf("  Preopen directory fd %d flags: 0x%x\n", preopen_fd, flags);
}

static void test_fdflags_vs_fdstat(void) {
  printf("\nTest 12: fd_fdflags_get vs fd_fdstat_get comparison\n");

  __wasi_fdflagsext_t fdflags;
  __wasi_fdstat_t fdstat;
  __wasi_errno_t ret;

  ret = __wasi_fd_fdflags_get(0, &fdflags);
  assert(ret == 0 && "fd_fdflags_get should succeed");

  ret = __wasi_fd_fdstat_get(0, &fdstat);
  assert(ret == 0 && "fd_fdstat_get should succeed");

  printf("  fd_fdflags_get returned: 0x%x\n", fdflags);
  printf("  fd_fdstat_get fs_flags: 0x%x\n", fdstat.fs_flags);
  printf(
      "  Both syscalls succeeded (values may differ - different flag types)\n");
}

int main(void) {
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
  printf("All fd_fdflags_get tests completed!\n");

  return 0;
}
