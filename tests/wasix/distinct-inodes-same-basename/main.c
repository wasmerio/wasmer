#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

static void fail(const char *message)
{
    perror(message);
    exit(1);
}

static void write_all(int fd, const char *contents)
{
    size_t remaining = strlen(contents);
    const char *cursor = contents;

    while (remaining > 0)
    {
        ssize_t written = write(fd, cursor, remaining);
        if (written < 0)
        {
            fail("write");
        }
        cursor += written;
        remaining -= (size_t)written;
    }
}

static void create_file(const char *path, const char *contents)
{
    int fd = open(path, O_CREAT | O_WRONLY | O_TRUNC, 0666);
    if (fd < 0)
    {
        fail(path);
    }

    write_all(fd, contents);

    if (close(fd) != 0)
    {
        fail("close");
    }
}

static void read_back(const char *path, char *buf, size_t len)
{
    int fd = open(path, O_RDONLY);
    if (fd < 0)
    {
        fail(path);
    }

    ssize_t nread = read(fd, buf, len - 1);
    if (nread < 0)
    {
        fail("read");
    }
    buf[nread] = '\0';

    if (close(fd) != 0)
    {
        fail("close");
    }
}

int main(void)
{
    struct stat src_stat;
    struct stat dst_stat;
    char src_buf[32];
    char dst_buf[32];

    if (mkdir("src", 0777) != 0)
    {
        fail("mkdir src");
    }
    if (mkdir("dst", 0777) != 0)
    {
        fail("mkdir dst");
    }

    create_file("src/file.txt", "source");
    create_file("dst/file.txt", "dest");

    if (stat("src/file.txt", &src_stat) != 0)
    {
        fail("stat src/file.txt");
    }
    if (stat("dst/file.txt", &dst_stat) != 0)
    {
        fail("stat dst/file.txt");
    }

    if (src_stat.st_ino == dst_stat.st_ino)
    {
        fprintf(stderr,
                "expected distinct inode ids, got src=%llu dst=%llu\n",
                (unsigned long long)src_stat.st_ino,
                (unsigned long long)dst_stat.st_ino);
        return 1;
    }

    read_back("src/file.txt", src_buf, sizeof(src_buf));
    read_back("dst/file.txt", dst_buf, sizeof(dst_buf));

    if (strcmp(src_buf, "source") != 0)
    {
        fprintf(stderr, "unexpected src contents: %s\n", src_buf);
        return 1;
    }
    if (strcmp(dst_buf, "dest") != 0)
    {
        fprintf(stderr, "unexpected dst contents: %s\n", dst_buf);
        return 1;
    }

    printf("0");
    return 0;
}
