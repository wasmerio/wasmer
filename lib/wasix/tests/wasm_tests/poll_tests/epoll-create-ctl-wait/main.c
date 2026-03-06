#include <stdio.h>
#include <stdlib.h>
#include <sys/epoll.h>
#include <sys/eventfd.h>
#include <unistd.h>
#include <stdint.h>

int main()
{
    int status = EXIT_FAILURE;

    int efd, epoll_fd;
    struct epoll_event event;
    struct epoll_event events[1];

    efd = eventfd(0, 0);
    if (efd == -1)
    {
        goto end;
    }

    epoll_fd = epoll_create1(0);
    if (epoll_fd == -1)
    {
        goto end;
    }

    event.events = EPOLLIN;
    event.data.fd = efd;
    if (epoll_ctl(epoll_fd, EPOLL_CTL_ADD, efd, &event) == -1)
    {
        goto end;
    }

    uint64_t val = 1;
    if (write(efd, &val, sizeof(uint64_t)) != sizeof(uint64_t))
    {
        goto end;
    }

    int n = epoll_wait(epoll_fd, events, 1, -1);
    if (n == -1)
    {
        goto end;;
    }

    status = EXIT_SUCCESS;

end:
    printf("%d", status);
    return status;
}
