#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static int create_file(const char *name)
{
    int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    return fd;
}

static void test_dupfd_minimum_available(void)
{
    // From LTP fcntl01.c: F_DUPFD returns the lowest available fd >= min.
    printf("Test 1: F_DUPFD returns minimum available\n");
    int fd = create_file("fd_dup2_min_file");
    int hole_fd = create_file("fd_dup2_min_file2");
    assert(close(hole_fd) == 0);

    int dup_fd = fcntl(fd, F_DUPFD, hole_fd);
    assert(dup_fd == hole_fd);

    assert(close(dup_fd) == 0);
    assert(close(fd) == 0);
    assert(unlink("fd_dup2_min_file") == 0);
    assert(unlink("fd_dup2_min_file2") == 0);
}

static void test_dupfd_minimums(void)
{
    // From LTP fcntl02.c and gVisor fcntl.cc: returned fd >= min.
    printf("Test 2: F_DUPFD respects minimums\n");
    static const int min_fds[] = {0, 1, 2, 3, 10, 100};

    int fd = create_file("fd_dup2_minimums");

    for (size_t i = 0; i < sizeof(min_fds) / sizeof(min_fds[0]); i++) {
        int min_fd = min_fds[i];
        int dup_fd = fcntl(fd, F_DUPFD, min_fd);
        assert(dup_fd >= min_fd);
        assert(dup_fd != fd);
        assert(close(dup_fd) == 0);
    }

    assert(close(fd) == 0);
    assert(unlink("fd_dup2_minimums") == 0);
}

static void test_dupfd_shared_offset(void)
{
    // Based on dup semantics: the duplicated fd shares file offset.
    printf("Test 3: F_DUPFD shares file offset\n");
    const char payload[] = "abcdef";
    char ch = '\0';

    int fd = create_file("fd_dup2_offset");
    assert(write(fd, payload, sizeof(payload)) == (ssize_t)sizeof(payload));
    assert(lseek(fd, 0, SEEK_SET) == 0);

    int dup_fd = fcntl(fd, F_DUPFD, 0);
    assert(dup_fd >= 0);
    assert(read(dup_fd, &ch, 1) == 1);
    assert(lseek(fd, 0, SEEK_CUR) == 1);

    assert(close(dup_fd) == 0);
    assert(close(fd) == 0);
    assert(unlink("fd_dup2_offset") == 0);
}

static void test_dupfd_cloexec(void)
{
    // From LTP fcntl29.c and gVisor fcntl.cc: F_DUPFD_CLOEXEC sets FD_CLOEXEC.
    printf("Test 4: F_DUPFD_CLOEXEC sets FD_CLOEXEC\n");
    int fd = create_file("fd_dup2_cloexec");
    int dup_fd = fcntl(fd, F_DUPFD_CLOEXEC, 0);
    assert(dup_fd >= 0);

    int flags = fcntl(dup_fd, F_GETFD);
    assert(flags >= 0);
    assert((flags & FD_CLOEXEC) != 0);

    int dup_fd2 = fcntl(fd, F_DUPFD, 0);
    assert(dup_fd2 >= 0);
    flags = fcntl(dup_fd2, F_GETFD);
    assert(flags >= 0);
    assert((flags & FD_CLOEXEC) == 0);

    flags = fcntl(fd, F_GETFD);
    assert(flags >= 0);
    assert((flags & FD_CLOEXEC) == 0);

    assert(close(dup_fd) == 0);
    assert(close(dup_fd2) == 0);
    assert(close(fd) == 0);
    assert(unlink("fd_dup2_cloexec") == 0);
}

static void test_dupfd_bad_fd(void)
{
    // From LTP fcntl12.c: invalid fd yields EBADF.
    printf("Test 5: F_DUPFD invalid fd\n");
    errno = 0;
    int ret = fcntl(-1, F_DUPFD, 0);
    assert(ret == -1);
    assert(errno == EBADF);
}

int main(void)
{
    test_dupfd_minimum_available();
    test_dupfd_minimums();
    test_dupfd_shared_offset();
    test_dupfd_cloexec();
    test_dupfd_bad_fd();
    printf("All tests passed!\n");
    return 0;
}
