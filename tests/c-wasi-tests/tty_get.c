#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

static void assert_isatty_matches(int fd, __wasi_bool_t is_tty, const char *label)
{
    int expected = is_tty == __WASI_BOOL_TRUE ? 1 : 0;
    errno = 0;
    int actual = isatty(fd);
    int saved_errno = errno;
    printf("  %s: tty=%u isatty=%d errno=%d\n",
           label,
           (unsigned)is_tty,
           actual,
           saved_errno);
    assert(actual == expected && "isatty should match tty_get state");
}

static void test_stdio_isatty_matches_tty_get(void)
{
    printf("Test 1: stdio isatty matches tty_get\n");

    __wasi_tty_t tty = {0};
    __wasi_errno_t err = __wasi_tty_get(&tty);
    assert(err == __WASI_ERRNO_SUCCESS);

    assert_isatty_matches(STDIN_FILENO, tty.stdin_tty, "stdin");
    assert_isatty_matches(STDOUT_FILENO, tty.stdout_tty, "stdout");
    assert_isatty_matches(STDERR_FILENO, tty.stderr_tty, "stderr");
}

static void test_regular_file_isatty(void)
{
    printf("Test 2: regular file is not a tty\n");

    int fd = open("tty_get_regular_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(fd > STDERR_FILENO);

    errno = 0;
    int ret = isatty(fd);
    assert(ret == 0);
    assert(errno == ENOTTY);

    assert(close(fd) == 0);
    assert(unlink("tty_get_regular_file") == 0);
}

int main(void)
{
    test_stdio_isatty_matches_tty_get();
    test_regular_file_isatty();
    printf("All tests passed!\n");
    return 0;
}
