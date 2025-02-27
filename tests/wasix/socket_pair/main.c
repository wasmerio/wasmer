// Note: we need this test because we're hacking around
// socket pairs and using a duplex pipe underneath, which
// creates huge potential for edge cases and errors.

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <unistd.h>
#include <errno.h>

int test_communication(int from, int to)
{
    int result;
    char buf[32];
    ssize_t numRead;
    fd_set fdset;
    struct timeval timeout = {
        .tv_sec = 0,
        .tv_usec = 0,
    };

    // Test 1: do it through select, with read and write
    FD_ZERO(&fdset);
    FD_SET(to, &fdset);
    result = select(to + 1, NULL, &fdset, NULL, &timeout);
    if (result < 0)
    {
        perror("select");
        return -1;
    }
    else if (result == 0)
    {
        printf("Timeout: nothing can be written.\n");
        return -1;
    }
    else
    {
        if (FD_ISSET(to, &fdset))
        {
            int bytes_written = write(to, "foo", 3);
            if (bytes_written < 0)
            {
                perror("write");
                return -1;
            }
        }
        else
        {
            printf("Expected send socket to be ready to write to\n");
            return -1;
        }
    }

    FD_ZERO(&fdset);
    FD_SET(from, &fdset);
    result = select(from + 1, &fdset, NULL, NULL, &timeout);
    if (result < 0)
    {
        perror("select");
        return -1;
    }
    else if (result == 0)
    {
        printf("Timeout: No data available to read.\n");
        return -1;
    }
    else
    {
        if (FD_ISSET(from, &fdset))
        {
            int bytes_read = read(from, buf, sizeof(buf));
            if (bytes_read < 0)
            {
                perror("read");
                return -1;
            }
            buf[bytes_read] = 0;
            if (strncmp(buf, "foo", 3) != 0)
            {
                printf("expected 'foo', received: %s\n", buf);
                return -1;
            }
        }
        else
        {
            printf("Expected recv socket to be ready to read from\n");
            return -1;
        }
    }

    // test 2: do it through send and recv

    int bytes_sent = send(to, "bar", 3, 0);
    if (bytes_sent < 0)
    {
        perror("send");
        return -1;
    }

    int bytes_received = recv(from, buf, sizeof(buf), 0);
    if (bytes_received < 0)
    {
        perror("recv");
        return -1;
    }
    buf[bytes_received] = 0;

    if (strncmp(buf, "bar", 3) != 0)
    {
        printf("expected 'bar', received: %s\n", buf);
        return -1;
    }

    return 0;
}

int main()
{
    int socks[2];

    if (socketpair(AF_UNIX, SOCK_STREAM, 0, socks) == -1)
    {
        perror("socketpair");
        return -1;
    }

    if (test_communication(socks[0], socks[1]) == -1)
    {
        return -1;
    }

    // try it in reverse as well, since the connection should be duplex
    if (test_communication(socks[1], socks[0]) == -1)
    {
        return -1;
    }

    return 0;
}