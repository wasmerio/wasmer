#include <stdio.h>
#include <stdlib.h>
#include <sys/epoll.h>
#include <sys/eventfd.h>
#include <unistd.h>
#include <stdint.h>

int main()
{
    int status = EXIT_FAILURE;
    int efd1 = -1, efd2 = -1, epoll_fd = -1;
    struct epoll_event event;
    struct epoll_event events[4];

    efd1 = eventfd(0, 0);
    if (efd1 == -1)
    {
        goto end;
    }

    efd2 = eventfd(0, 0);
    if (efd2 == -1)
    {
        goto end;
    }

    epoll_fd = epoll_create1(0);
    if (epoll_fd == -1)
    {
        goto end;
    }

    event.events = EPOLLIN;
    event.data.fd = efd1;
    if (epoll_ctl(epoll_fd, EPOLL_CTL_ADD, efd1, &event) == -1)
    {
        goto end;
    }

    event.events = EPOLLIN;
    event.data.fd = efd2;
    if (epoll_ctl(epoll_fd, EPOLL_CTL_ADD, efd2, &event) == -1)
    {
        goto end;
    }

    uint64_t val = 1;
    if (write(efd1, &val, sizeof(uint64_t)) != sizeof(uint64_t))
    {
        goto end;
    }
    if (write(efd2, &val, sizeof(uint64_t)) != sizeof(uint64_t))
    {
        goto end;
    }

    int n = epoll_wait(epoll_fd, events, 2, 1000);
    if (n != 2)
    {
        goto end;
    }
    int seen_efd1 = 0;
    int seen_efd2 = 0;
    for (int i = 0; i < n; i++)
    {
        if (events[i].data.fd == efd1)
            seen_efd1 = 1;
        if (events[i].data.fd == efd2)
            seen_efd2 = 1;
    }
    if (!seen_efd1 || !seen_efd2)
    {
        goto end;
    }

    if (epoll_ctl(epoll_fd, EPOLL_CTL_DEL, efd1, NULL) == -1)
    {
        goto end;
    }

    if (write(efd1, &val, sizeof(uint64_t)) != sizeof(uint64_t))
    {
        goto end;
    }
    if (write(efd2, &val, sizeof(uint64_t)) != sizeof(uint64_t))
    {
        goto end;
    }

    n = epoll_wait(epoll_fd, events, 4, 1000);
    if (n <= 0)
    {
        goto end;
    }
    for (int i = 0; i < n; i++)
    {
        if (events[i].data.fd == efd1)
        {
            goto end;
        }
    }

    event.events = EPOLLOUT;
    event.data.fd = efd2;
    if (epoll_ctl(epoll_fd, EPOLL_CTL_MOD, efd2, &event) == -1)
    {
        goto end;
    }

    n = epoll_wait(epoll_fd, events, 4, 1000);
    if (n <= 0)
    {
        goto end;
    }
    int seen_out = 0;
    for (int i = 0; i < n; i++)
    {
        if (events[i].data.fd == efd1)
        {
            goto end;
        }
        if (events[i].data.fd == efd2)
        {
            if ((events[i].events & EPOLLOUT) != 0)
                seen_out = 1;
            if ((events[i].events & EPOLLIN) != 0)
            {
                goto end;
            }
        }
    }
    if (!seen_out)
    {
        goto end;
    }

    status = EXIT_SUCCESS;

end:
    if (efd1 != -1)
        close(efd1);
    if (efd2 != -1)
        close(efd2);
    if (epoll_fd != -1)
        close(epoll_fd);
    printf("%d", status);
    return status;
}
