// Keep a real listening socket across exec so ss must enumerate it and its owner.
#include <assert.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    assert(fd >= 0);
    struct sockaddr_in address = {
        .sin_family = AF_INET,
        .sin_port = htons(31337),
        .sin_addr.s_addr = htonl(INADDR_LOOPBACK),
    };
    assert(bind(fd, (void *)&address, sizeof(address)) == 0);
    assert(listen(fd, 4) == 0);
    execl("/bin/ss", "ss", "-lntp", (char *)0);
    return 1;
}
