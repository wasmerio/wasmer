//#ExpectedStdout: TCP OOB send, readiness, peek, and receive work
//#MinimalLibc: v2026-08-07.1
//#UnixOnly: true

#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

static int connected_tcp_pair(int* sender, int* receiver) {
  int listener = -1;
  struct sockaddr_in addr;
  socklen_t addr_len = sizeof(addr);

  listener = socket(AF_INET, SOCK_STREAM, 0);
  if (listener < 0) {
    perror("socket(listener)");
    return -1;
  }

  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr.sin_port = htons(0);
  if (bind(listener, (struct sockaddr*)&addr, sizeof(addr)) < 0 ||
      listen(listener, 1) < 0 ||
      getsockname(listener, (struct sockaddr*)&addr, &addr_len) < 0) {
    perror("prepare listener");
    close(listener);
    return -1;
  }

  *sender = socket(AF_INET, SOCK_STREAM, 0);
  if (*sender < 0 ||
      connect(*sender, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
    perror("connect(sender)");
    if (*sender >= 0) close(*sender);
    close(listener);
    return -1;
  }

  *receiver = accept(listener, NULL, NULL);
  close(listener);
  if (*receiver < 0) {
    perror("accept(receiver)");
    close(*sender);
    return -1;
  }
  return 0;
}

static int wait_for_select_read_and_except(int fd) {
  struct timespec retry_delay = {.tv_sec = 0, .tv_nsec = 10000000};

  for (int attempt = 0; attempt < 100; ++attempt) {
    fd_set readfds;
    fd_set exceptfds;
    struct timeval timeout = {.tv_sec = 0, .tv_usec = 0};
    FD_ZERO(&readfds);
    FD_ZERO(&exceptfds);
    FD_SET(fd, &readfds);
    FD_SET(fd, &exceptfds);

    int ready = select(fd + 1, &readfds, NULL, &exceptfds, &timeout);
    if (ready < 0) {
      perror("select");
      return -1;
    }
    if (FD_ISSET(fd, &readfds) && FD_ISSET(fd, &exceptfds)) return 0;
    nanosleep(&retry_delay, NULL);
  }

  fprintf(stderr, "select never reported both readable and exceptional\n");
  return -1;
}

static int expect_epollpri(int fd) {
  int epoll_fd = epoll_create1(0);
  if (epoll_fd < 0) {
    perror("epoll_create1");
    return -1;
  }

  struct epoll_event registration = {.events = EPOLLPRI, .data.fd = fd};
  if (epoll_ctl(epoll_fd, EPOLL_CTL_ADD, fd, &registration) < 0) {
    perror("epoll_ctl(EPOLLPRI)");
    close(epoll_fd);
    return -1;
  }

  struct epoll_event event;
  int count = epoll_wait(epoll_fd, &event, 1, 1000);
  close(epoll_fd);
  if (count != 1 || (event.events & EPOLLPRI) == 0) {
    fprintf(stderr, "epoll did not report EPOLLPRI (count=%d events=%u)\n",
            count, count == 1 ? event.events : 0);
    return -1;
  }
  return 0;
}

int main(void) {
  int sender = -1;
  int receiver = -1;
  int result = 1;
  char normal[sizeof("before") - 1];
  char urgent = '\0';

  if (connected_tcp_pair(&sender, &receiver) < 0) goto cleanup;
  if (send(sender, "before", sizeof(normal), 0) != sizeof(normal)) {
    perror("send(normal)");
    goto cleanup;
  }
  if (send(sender, "!", 1, MSG_OOB) != 1) {
    perror("send(MSG_OOB)");
    goto cleanup;
  }

  /* select() exercises read readiness and exceptfds together. The readiness
   * check must not consume normal data or destroy the pending urgent byte. */
  if (wait_for_select_read_and_except(receiver) < 0) goto cleanup;
  if (expect_epollpri(receiver) < 0) goto cleanup;

  if (recv(receiver, &urgent, 1, MSG_OOB | MSG_PEEK) != 1 || urgent != '!') {
    perror("recv(MSG_OOB | MSG_PEEK)");
    goto cleanup;
  }
  urgent = '\0';
  if (recv(receiver, &urgent, 1, MSG_OOB) != 1 || urgent != '!') {
    perror("recv(MSG_OOB)");
    goto cleanup;
  }
  if (recv(receiver, normal, sizeof(normal), 0) != sizeof(normal) ||
      memcmp(normal, "before", sizeof(normal)) != 0) {
    perror("recv(normal)");
    goto cleanup;
  }

  puts("TCP OOB send, readiness, peek, and receive work");
  result = 0;

cleanup:
  if (receiver >= 0) close(receiver);
  if (sender >= 0) close(sender);
  return result;
}
