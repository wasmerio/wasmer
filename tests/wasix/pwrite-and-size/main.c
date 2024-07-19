#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>

int main()
{
    int fd;
    char *data;
    struct stat statbuf;

    fd = open("/data/my_file.txt", O_CREAT | O_WRONLY, 0644);
    if (fd == -1)
    {
        goto fail;
    }

    data = "ABC";

    if (pwrite(fd, data, 3, 0) == -1)
    {
        goto fail;
    }
    if (fstat(fd, &statbuf) == -1)
    {
        goto fail;
    }

    if (statbuf.st_size != 3)
    {
        goto fail;
    }

    data = "D";
    if (pwrite(fd, data, 1, 1) == -1)
    {
        goto fail;
    }

    if (fstat(fd, &statbuf) == -1)
    {
        goto fail;
    }
    if (statbuf.st_size != 3)
    {
        goto fail;
    }

    data = "XYZ";
    if (pwrite(fd, data, 3, 3) == -1)
    {
        goto fail;
    }
    if (fstat(fd, &statbuf) == -1)
    {
        goto fail;
    }
    if (statbuf.st_size != 6)
    {
        goto fail;
    }

    data = "GHIJKLMJ";
    if (pwrite(fd, data, 10, 1) == -1)
    {
        goto fail;
    }
    if (fstat(fd, &statbuf) == -1)
    {
        goto fail;
    }

    if (statbuf.st_size != 11)
    {
        goto fail;
    }

    printf("0");

    return 0;

fail:
    printf("1");

    return 1;
}
