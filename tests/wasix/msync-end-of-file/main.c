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

    fd = open("/data/my_file.txt", O_RDWR | O_CREAT, S_IRUSR | S_IWUSR);
    if (fd == -1)
    {
        printf("open");
        exit(1);
    }

    write(fd, "abcdef", 6);

    struct stat statbuf;
    fstat(fd, &statbuf);
    size_t filesize = statbuf.st_size;

    data = mmap(NULL, 3, PROT_READ | PROT_WRITE, MAP_PRIVATE, fd, 3);
    if (data == MAP_FAILED)
    {
        printf("mmap");
        exit(1);
    }

    memcpy(data, "hij", 3);

    msync(data, 3, MS_SYNC);

    munmap(data, 3);

    fd = open("/data/my_file.txt", O_RDONLY);
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

    if (strncmp(buffer, "abchij", filesize) != 0)
    {
        printf("Error: Expected content 'abchij', got '%s'\n", buffer);
        exit(1);
    }

    printf("0");
    close(fd);
    return 0;
}
