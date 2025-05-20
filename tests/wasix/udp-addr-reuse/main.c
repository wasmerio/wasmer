#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

/*
this is an example of address reuse using UDP sockets
it set address reuse option to 1 and then bind to the same address
it set port reuse option to 1 and then bind to the same port
*/
//#ifndef SO_REUSEPORT
//#define SO_REUSEPORT 15
//#endif


int main(int argc, char *argv[])
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

    printf("%d", EXIT_SUCCESS);
    return EXIT_SUCCESS;
    
}
