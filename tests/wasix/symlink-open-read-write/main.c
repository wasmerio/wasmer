#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static void fail(const char *msg)
{
    perror(msg);
    exit(1);
}

int main(void)
{
    const char *target = "/host/target.txt";
    const char *linkname = "hello";
    const char *nested_dir = "nested";
    const char *nested_linkname = "nested/hello";
    const char *suffix = " bla";
    char prefix[128] = {0};
    char buf[256] = {0};

    unlink(linkname);
    unlink(nested_linkname);
    if (mkdir(nested_dir, 0777) != 0 && errno != EEXIST) {
        fail("mkdir nested");
    }

    if (symlink(target, linkname) != 0) {
        fail("symlink");
    }
    if (symlink(target, nested_linkname) != 0) {
        fail("symlink nested");
    }

    int fd = open(target, O_RDONLY);
    if (fd < 0) {
        fail("open target for initial read");
    }
    ssize_t n = read(fd, prefix, sizeof(prefix) - 1);
    if (n < 0) {
        fail("read initial target");
    }
    if (close(fd) != 0) {
        fail("close initial target fd");
    }
    prefix[n] = '\0';

    fd = open(linkname, O_WRONLY | O_APPEND);
    if (fd < 0) {
        fail("open symlink for append");
    }
    if (write(fd, suffix, strlen(suffix)) < 0) {
        fail("append through symlink");
    }
    if (close(fd) != 0) {
        fail("close symlink fd");
    }

    fd = open(linkname, O_RDONLY);
    if (fd < 0) {
        fail("open symlink for read");
    }
    n = read(fd, buf, sizeof(buf) - 1);
    if (n < 0) {
        fail("read through symlink");
    }
    if (close(fd) != 0) {
        fail("close symlink read fd");
    }
    buf[n] = '\0';

    char expected[256] = {0};
    snprintf(expected, sizeof(expected), "%s%s", prefix, suffix);

    if (strcmp(buf, expected) != 0) {
        fprintf(stderr, "unexpected symlink content: '%s'\n", buf);
        return 1;
    }

    memset(buf, 0, sizeof(buf));
    fd = open(nested_linkname, O_RDONLY);
    if (fd < 0) {
        fail("open nested symlink for read");
    }
    n = read(fd, buf, sizeof(buf) - 1);
    if (n < 0) {
        fail("read through nested symlink");
    }
    if (close(fd) != 0) {
        fail("close nested symlink read fd");
    }
    buf[n] = '\0';

    if (strcmp(buf, expected) != 0) {
        fprintf(stderr, "unexpected nested symlink content: '%s'\n", buf);
        return 1;
    }

    memset(buf, 0, sizeof(buf));
    fd = open(target, O_RDONLY);
    if (fd < 0) {
        fail("open target for verify");
    }
    n = read(fd, buf, sizeof(buf) - 1);
    if (n < 0) {
        fail("read target");
    }
    if (close(fd) != 0) {
        fail("close target read fd");
    }
    buf[n] = '\0';

    if (strcmp(buf, expected) != 0) {
        fprintf(stderr, "unexpected target content: '%s'\n", buf);
        return 1;
    }

    printf("0");
    return 0;
}
