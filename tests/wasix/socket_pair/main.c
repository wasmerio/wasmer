#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <unistd.h>

int main()
{
    int status = EXIT_FAILURE;

    int socks[2];
    char buf[1024];
    ssize_t numRead;

    if (socketpair(AF_UNIX, SOCK_STREAM, 0, socks) == -1)
    {
        perror("socketpair");
        goto end;
    }

    if (write(socks[0], "foo", 3) == -1)
    {
        perror("write");
        goto end;
    }

    memset(buf, 0, sizeof(buf));
    numRead = read(socks[1], buf, sizeof(buf));
    if (numRead == -1)
    {
        perror("read");
        goto end;
    }
    if (strncmp(buf, "foo", 3) != 0)
    {
        printf("buf: %s\n", buf);
        goto end;
    }

    if (write(socks[1], "bar", 3) == -1)
    {
        perror("write 2");
        goto end;
    }

    memset(buf, 0, sizeof(buf));
    numRead = read(socks[0], buf, sizeof(buf));
    if (numRead == -1)
    {
        perror("read 2");
        goto end;
    }
    if (strncmp(buf, "bar", 3) != 0)
    {
        printf("buf 2: %s\n", buf);
        goto end;
    }

    status = EXIT_SUCCESS;

end:
    printf("%d", status);
    return status;
}
