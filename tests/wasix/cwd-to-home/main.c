#include <stdio.h>
#include <fcntl.h>

int main()
{
    int fd = open("hello.txt", O_RDWR);
    printf("%d", (fd == -1));

    return 0;
}