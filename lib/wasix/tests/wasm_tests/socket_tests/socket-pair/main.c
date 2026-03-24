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
#include <fcntl.h>
#include <pthread.h>
#include <memory.h>
#include <sys/epoll.h>

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

#define BUFFER_SIZE 1024
#define MAX_EVENTS 10

typedef struct
{
    int fd;
    char buffer[BUFFER_SIZE];
} socket_data_t;

void *epoll_thread(void *arg)
{
    struct epoll_event events[MAX_EVENTS];
    int n, i;
    int epoll_fd = *(int *)arg;
    // Quick'n'dirty way to read exactly 3 packets
    int num_events = 0;
    int sum = 0;

    while (1)
    {
        n = epoll_wait(epoll_fd, events, MAX_EVENTS, 5000);
        if (n == -1)
        {
            perror("epoll_wait");
            return (void *)0;
        }
        else if (n == 0)
        {
            printf("No events occurred within the timeout period.\n");
            return (void *)0;
        }

        for (i = 0; i < n; i++)
        {
            socket_data_t *data = (socket_data_t *)events[i].data.ptr;

            if (events[i].events & EPOLLIN)
            {
                num_events++;

                while (1)
                {
                    ssize_t count = read(data->fd, data->buffer, BUFFER_SIZE - 1);

                    if (count == -1)
                    {
                        if (errno != EAGAIN)
                        {
                            perror("read error");
                            close(data->fd);
                            free(data);
                            return (void *)0;
                        }
                        break;
                    }
                    else if (count == 0)
                    {
                        // Connection closed by client
                        close(data->fd);
                        free(data);
                        return (void *)0;
                    }

                    // Super-hacky, relies on packets arriving all at once, but
                    // that should be the case anyway
                    data->buffer[count] = '\0';
                    sum += atoi(data->buffer);

                    // Reset for next read
                    memset(data->buffer, 0, BUFFER_SIZE);
                }

                if (num_events >= 3)
                {
                    close(data->fd);
                    free(data);
                    return (void *)sum;
                }
            }
            else
            {
                printf("Unexpected event on fd %d\n", data->fd);
                close(data->fd);
                free(data);
                return (void *)0;
            }
        }
    }
}

int wait_via_epoll()
{
    int epoll_fd = epoll_create1(0);
    if (epoll_fd == -1)
    {
        perror("epoll_create1");
        exit(EXIT_FAILURE);
    }

    int sockfd[2];
    if (socketpair(AF_UNIX, SOCK_STREAM, 0, sockfd) == -1)
    {
        perror("socketpair");
        close(epoll_fd);
        exit(EXIT_FAILURE);
    }

    int recv_fd = sockfd[0];
    int send_fd = sockfd[1];

    int flags = fcntl(recv_fd, F_GETFL, 0);
    fcntl(recv_fd, F_SETFL, flags | O_NONBLOCK);

    socket_data_t *data = malloc(sizeof(socket_data_t));
    if (!data)
    {
        perror("malloc");
        return -1;
    }

    data->fd = recv_fd;
    memset(data->buffer, 0, BUFFER_SIZE);

    struct epoll_event event;
    event.events = EPOLLIN | EPOLLET; // Edge-triggered mode
    event.data.ptr = data;

    if (epoll_ctl(epoll_fd, EPOLL_CTL_ADD, recv_fd, &event) == -1)
    {
        perror("epoll_ctl failed");
        free(data);
        return -1;
    }

    pthread_attr_t attr;
    pthread_attr_init(&attr);
    pthread_t thread;
    pthread_create(&thread, &attr, epoll_thread, (void *)&epoll_fd);

    struct timespec ts;
    ts.tv_sec = 0;
    ts.tv_nsec = 100000000;

    send(send_fd, "42", 2, 0);
    nanosleep(&ts, NULL);
    send(send_fd, "69", 2, 0);
    nanosleep(&ts, NULL);
    send(send_fd, "85", 2, 0);
    nanosleep(&ts, NULL);
    close(send_fd);

    void *result;
    pthread_join(thread, &result);
    int sum = (int)result;
    if (sum == 0)
    {
        printf("Error in epoll thread\n");
        close(epoll_fd);
        return -1;
    }
    else if (sum != 196)
    {
        printf("Expected sum to be 196, got %d\n", sum);
        close(epoll_fd);
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

    if (wait_via_epoll() != 0)
    {
        return -1;
    }

    return 0;
}