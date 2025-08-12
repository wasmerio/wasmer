#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <string.h>

/*
this is an example of address reuse using UDP sockets
it set address reuse option to 1 and then bind to the same address
it set port reuse option to 1 and then bind to the same port
*/
int test_addr_reuse()
{
    int sock1, sock2;
    int reuse = 1;
    struct sockaddr_in addr;

    sock1 = socket(AF_INET, SOCK_DGRAM, 0);
    if (sock1 < 0)
    {
        puts("failed to create the first socket");
        return EXIT_FAILURE;
    }

    if (setsockopt(sock1, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse)) < 0)
    {
        puts("setsockopt first socket SO_REUSEADDR failed");
        return EXIT_FAILURE;
    }

    if (setsockopt(sock1, SOL_SOCKET, SO_REUSEPORT, &reuse, sizeof(reuse)) < 0)
    {
        puts("setsockopt first socket SO_REUSEPORT failed");
        return EXIT_FAILURE;
    }

    addr.sin_family = AF_INET;
    addr.sin_port = htons(12345);
    addr.sin_addr.s_addr = htonl(INADDR_ANY);

    if (bind(sock1, (struct sockaddr *)&addr, sizeof(addr)) < 0)
    {
        puts("first socket bind failed");
        return EXIT_FAILURE;
    }

    sock2 = socket(AF_INET, SOCK_DGRAM, 0);
    if (sock2 < 0)
    {
        puts("failed to create the second socket");
        return EXIT_FAILURE;
    }

    if (setsockopt(sock2, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse)) < 0)
    {
        puts("setsockopt second socket SO_REUSEADDR failed");
        return EXIT_FAILURE;
    }

    if (setsockopt(sock2, SOL_SOCKET, SO_REUSEPORT, &reuse, sizeof(reuse)) < 0)
    {
        puts("setsockopt second socket SO_REUSEPORT failed");
        return EXIT_FAILURE;
    }

    if (bind(sock2, (struct sockaddr *)&addr, sizeof(addr)) < 0)
    {
        puts("second socket bind failed");
        return EXIT_FAILURE;
    }

    close(sock1);
    close(sock2);

    return EXIT_SUCCESS;
}

int test_ipv6()
{
    int sock;
    struct sockaddr_in6 addr;

    sock = socket(AF_INET6, SOCK_DGRAM, 0);
    if (sock < 0)
    {
        puts("failed to create the socket");
        return EXIT_FAILURE;
    }

    addr.sin6_family = AF_INET6;
    addr.sin6_port = htons(0);
    addr.sin6_addr = in6addr_any;

    if (bind(sock, (struct sockaddr *)&addr, sizeof(addr)) < 0)
    {
        puts("socket bind failed");
        close(sock);
        return EXIT_FAILURE;
    }

    close(sock);
    return EXIT_SUCCESS;
}

int test_autobind_connect()
{
    int sock;
    struct sockaddr_in addr;

    sock = socket(AF_INET, SOCK_DGRAM, 0);
    if (sock < 0)
    {
        puts("failed to create the socket");
        return EXIT_FAILURE;
    }

    addr.sin_family = AF_INET;
    addr.sin_port = htons(65535);
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (connect(sock, (struct sockaddr *)&addr, sizeof(addr)) < 0)
    {
        puts("socket connect failed");
        close(sock);
        return EXIT_FAILURE;
    }

    close(sock);
    return EXIT_SUCCESS;
}

int test_autobind_sendto()
{
    int sock;
    struct sockaddr_in addr;

    sock = socket(AF_INET, SOCK_DGRAM, 0);
    if (sock < 0)
    {
        puts("failed to create the socket");
        return EXIT_FAILURE;
    }

    addr.sin_family = AF_INET;
    addr.sin_port = htons(65535);
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (sendto(sock, "hello", 5, 0, (struct sockaddr *)&addr, sizeof(addr)) < 0)
    {
        puts("sendto failed");
        close(sock);
        return EXIT_FAILURE;
    }

    close(sock);
    return EXIT_SUCCESS;
}

int main(int argc, char *argv[])
{
    if (argc < 2)
    {
        return EXIT_FAILURE;
    }

    if (!strcmp(argv[1], "addr-reuse"))
    {
        return test_addr_reuse();
    }
    else if (!strcmp(argv[1], "ipv6"))
    {
        return test_ipv6();
    }
    else if (!strcmp(argv[1], "autobind-connect"))
    {
        return test_autobind_connect();
    }
    else if (!strcmp(argv[1], "autobind-sendto"))
    {
        return test_autobind_sendto();
    }
    else
    {
        puts("Unknown test case");
        return EXIT_FAILURE;
    }
}