#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <sys/mman.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>

int main()
{
    int fd;
    char *data;

    fd = open("data/my_file.txt", O_RDWR | O_CREAT, S_IRUSR | S_IWUSR);
    if (fd == -1)
    {
        printf("open");
        exit(1);
    }

    write(fd, "abcdef", 6);

    struct stat statbuf;
    fstat(fd, &statbuf);
    size_t filesize = statbuf.st_size;

    data = mmap(NULL, 2, PROT_READ | PROT_WRITE, MAP_PRIVATE, fd, 2);
    if (data == MAP_FAILED)
    {
        printf("mmap");
        exit(1);
    }

    memcpy(data, "hi", 2);

    msync(data, 2, MS_SYNC);

    munmap(data, 2);

    fd = open("data/my_file.txt", O_RDONLY);
    if (fd == -1)
    {
        printf("open");
        exit(1);
    }

    char buffer[filesize];
    ssize_t bytes_read = read(fd, buffer, filesize);
    if (bytes_read == -1)
    {
        printf("read");
        exit(1);
    }

    if (strncmp(buffer, "abhief", filesize) != 0)
    {
        printf("Error: Expected content 'abhief', got '%s'\n", buffer);
        exit(1);
    }

    printf("0");
    close(fd);
    return 0;
}
