#include <assert.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static void test_pollout(void)
{
    printf("Test 1: POLLOUT on pipe write end\n");
    int fds[2];
    assert(pipe(fds) == 0);

    struct pollfd pfd = {.fd = fds[1], .events = POLLOUT, .revents = 0};
    int ret = poll(&pfd, 1, -1);
    assert(ret == 1);
    assert((pfd.revents & POLLOUT) == POLLOUT);

    close(fds[0]);
    close(fds[1]);
}

static void test_pollin(void)
{
    printf("Test 2: POLLIN after write\n");
    int fds[2];
    assert(pipe(fds) == 0);

    const char msg[] = "Testing";
    assert(write(fds[1], msg, sizeof(msg)) == (ssize_t)sizeof(msg));

    struct pollfd pfd = {.fd = fds[0], .events = POLLIN, .revents = 0};
    int ret = poll(&pfd, 1, -1);
    assert(ret == 1);
    assert((pfd.revents & POLLIN) == POLLIN);

    char buf[16] = {0};
    assert(read(fds[0], buf, sizeof(msg)) == (ssize_t)sizeof(msg));

    close(fds[0]);
    close(fds[1]);
}

static void test_timeout(void)
{
    printf("Test 3: poll timeout\n");
    int fds[2];
    assert(pipe(fds) == 0);

    struct pollfd pfd = {.fd = fds[0], .events = POLLIN, .revents = 0};
    int ret = poll(&pfd, 1, 50);
    assert(ret == 0);
    assert(pfd.revents == 0);

    close(fds[0]);
    close(fds[1]);
}

int main(void)
{
    printf("WASI poll_oneoff (poll/ppoll) integration tests\n");
    test_pollout();
    test_pollin();
    test_timeout();
    printf("All tests passed!\n");
    return 0;
}
