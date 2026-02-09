#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <termios.h>
#include <unistd.h>

#include <wasi/api_wasix.h>

static int failures = 0;

static void check(int cond, const char *msg)
{
    if (!cond) {
        fprintf(stderr, "FAIL: %s\n", msg);
        failures++;
    }
}

static void check_errno(int rc, int expected, const char *msg)
{
    if (rc != -1 || errno != expected) {
        fprintf(stderr, "FAIL: %s (rc=%d errno=%d expected=%d)\n",
                msg,
                rc,
                errno,
                expected);
        failures++;
    }
}

static void test_invalid_actions(void)
{
    printf("Test 1: tcsetattr invalid actions\n");
    struct termios tio;
    memset(&tio, 0, sizeof(tio));

    errno = 0;
    check_errno(tcsetattr(STDIN_FILENO, -1, &tio), EINVAL, "act=-1 should be EINVAL");

    errno = 0;
    check_errno(tcsetattr(STDIN_FILENO, 3, &tio), EINVAL, "act=3 should be EINVAL");
}

static void test_isatty_behavior(void)
{
    printf("Test 2: tcgetattr/tcsetattr honor isatty\n");
    struct termios tio;

    int tty = isatty(STDIN_FILENO);
    errno = 0;
    int rc = tcgetattr(STDIN_FILENO, &tio);
    if (tty) {
        check(rc == 0, "tcgetattr should succeed on tty stdin");
    } else {
        check_errno(rc, ENOTTY, "tcgetattr should fail with ENOTTY on non-tty stdin");
        return;
    }

    tio.c_lflag ^= ECHO;
    errno = 0;
    rc = tcsetattr(STDIN_FILENO, TCSANOW, &tio);
    check(rc == 0, "tcsetattr should succeed on tty stdin");
}

static void test_roundtrip_flags(void)
{
    printf("Test 3: tcsetattr/tcgetattr round-trip flags\n");
    struct termios orig;
    struct termios set;
    struct termios got;

    if (!isatty(STDIN_FILENO)) {
        printf("  stdin not tty, skipping round-trip\n");
        return;
    }

    errno = 0;
    check(tcgetattr(STDIN_FILENO, &orig) == 0, "tcgetattr should succeed on tty");

    set = orig;
    set.c_lflag ^= ECHO;
    set.c_lflag ^= ICANON;
    set.c_lflag ^= IGNCR;

    errno = 0;
    check(tcsetattr(STDIN_FILENO, TCSANOW, &set) == 0, "tcsetattr(TCSANOW) should succeed");
    errno = 0;
    check(tcsetattr(STDIN_FILENO, TCSADRAIN, &set) == 0, "tcsetattr(TCSADRAIN) should succeed");
    errno = 0;
    check(tcsetattr(STDIN_FILENO, TCSAFLUSH, &set) == 0, "tcsetattr(TCSAFLUSH) should succeed");

    errno = 0;
    check(tcgetattr(STDIN_FILENO, &got) == 0, "tcgetattr should succeed after set");
    check((got.c_lflag & ECHO) == (set.c_lflag & ECHO), "ECHO should round-trip");
    check((got.c_lflag & ICANON) == (set.c_lflag & ICANON), "ICANON should round-trip");
    check((got.c_lflag & IGNCR) == (set.c_lflag & IGNCR), "IGNCR should round-trip");

    __wasi_tty_t tty;
    __wasi_errno_t terr = __wasi_tty_get(&tty);
    check(terr == __WASI_ERRNO_SUCCESS, "__wasi_tty_get should succeed");
    check((tty.echo == __WASI_BOOL_TRUE) == ((set.c_lflag & ECHO) != 0),
          "tty.echo should match ECHO");
    check((tty.line_buffered == __WASI_BOOL_TRUE) == ((set.c_lflag & ICANON) != 0),
          "tty.line_buffered should match ICANON");
    check((tty.line_feeds == __WASI_BOOL_TRUE) == ((set.c_lflag & IGNCR) != 0),
          "tty.line_feeds should match IGNCR");

    errno = 0;
    check(tcsetattr(STDIN_FILENO, TCSANOW, &orig) == 0, "tcsetattr restore should succeed");
}

static void test_tty_line_feeds_roundtrip(void)
{
    printf("Test 4: __wasi_tty_set/get line_feeds round-trip\n");
    if (!isatty(STDIN_FILENO) || !isatty(STDOUT_FILENO) || !isatty(STDERR_FILENO)) {
        fprintf(stderr,
                "\n==============================\n"
                "WARNING: tty_set line_feeds round-trip requires an interactive TTY.\n"
                "Skipping this test in non-interactive mode.\n"
                "==============================\n");
        return;
    }
    __wasi_tty_t orig;
    __wasi_errno_t err = __wasi_tty_get(&orig);
    check(err == __WASI_ERRNO_SUCCESS, "__wasi_tty_get should succeed");

    __wasi_tty_t set = orig;
    set.line_feeds = (orig.line_feeds == __WASI_BOOL_TRUE) ? __WASI_BOOL_FALSE : __WASI_BOOL_TRUE;
    err = __wasi_tty_set(&set);
    check(err == __WASI_ERRNO_SUCCESS, "__wasi_tty_set should succeed");

    __wasi_tty_t got;
    memset(&got, 0, sizeof(got));
    err = __wasi_tty_get(&got);
    check(err == __WASI_ERRNO_SUCCESS, "__wasi_tty_get after set should succeed");
    check(got.line_feeds == set.line_feeds,
          "tty.line_feeds should round-trip via __wasi_tty_set/get");

    err = __wasi_tty_set(&orig);
    check(err == __WASI_ERRNO_SUCCESS, "restore tty state should succeed");
}

static void test_extproc_icanon(void)
{
    printf("Test 5: tcsetattr EXTPROC|ICANON\n");
    struct termios tio;

    if (!isatty(STDIN_FILENO)) {
        printf("  stdin not tty, skipping EXTPROC\n");
        return;
    }

    errno = 0;
    check(tcgetattr(STDIN_FILENO, &tio) == 0, "tcgetattr should succeed on tty");
    tio.c_lflag |= EXTPROC | ICANON;
    errno = 0;
    check(tcsetattr(STDIN_FILENO, TCSANOW, &tio) == 0, "tcsetattr EXTPROC|ICANON should succeed");
}

static void test_non_tty_fd(void)
{
    printf("Test 6: tcsetattr on non-tty fd\n");
    int fd = open("tty_set_regular_file", O_CREAT | O_TRUNC | O_RDWR, 0644);
    check(fd >= 0, "open regular file should succeed");

    struct termios tio;
    memset(&tio, 0, sizeof(tio));
    errno = 0;
    check_errno(tcsetattr(fd, TCSANOW, &tio), ENOTTY, "tcsetattr non-tty should be ENOTTY");

    errno = 0;
    check_errno(tcgetattr(fd, &tio), ENOTTY, "tcgetattr non-tty should be ENOTTY");

    close(fd);
    unlink("tty_set_regular_file");
}

static void test_invalid_fd(void)
{
    printf("Test 7: tcsetattr on invalid fd\n");
    struct termios tio;
    memset(&tio, 0, sizeof(tio));

    errno = 0;
    check_errno(tcsetattr(-1, TCSANOW, &tio), EBADF, "tcsetattr(-1) should be EBADF");
    errno = 0;
    check_errno(tcgetattr(-1, &tio), EBADF, "tcgetattr(-1) should be EBADF");
}

int main(void)
{
    test_invalid_actions();
    test_isatty_behavior();
    test_roundtrip_flags();
    test_tty_line_feeds_roundtrip();
    test_extproc_icanon();
    test_non_tty_fd();
    test_invalid_fd();
    assert(failures == 0);
    printf("All tests passed!\n");
    return 0;
}
