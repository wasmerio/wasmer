#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/socket.h>
#include <fcntl.h>

int main()
{
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0)
    {
        perror("socket");
        return 1;
    }

    if (close(fd) < 0)
    {
        perror("socket close");
        return 1;
    }

    fd = open("/bin", O_RDONLY | O_DIRECTORY);
    if (fd < 0)
    {
        perror("open dir");
        return 1;
    }

    if (close(fd) < 0)
    {
        perror("dir close");
        return 1;
    }

    return 0;
}