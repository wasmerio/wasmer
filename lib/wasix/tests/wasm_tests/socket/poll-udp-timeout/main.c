//#ExpectedStdout: UDP socket poll timeouts behaved as expected

#include <arpa/inet.h>
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

static long elapsed_ms(struct timespec start, struct timespec end) {
  time_t sec = end.tv_sec - start.tv_sec;
  long nsec = end.tv_nsec - start.tv_nsec;
  if (nsec < 0) {
    sec--;
    nsec += 1000000000L;
  }
  return sec * 1000 + nsec / 1000000L;
}

static int bind_loopback_udp(int fd, struct sockaddr_in* addr) {
  memset(addr, 0, sizeof(*addr));
  addr->sin_family = AF_INET;
  addr->sin_addr.s_addr = htonl(INADDR_LOOPBACK);
  addr->sin_port = 0;

  if (bind(fd, (struct sockaddr*)addr, sizeof(*addr)) != 0) {
    perror("bind");
    return 1;
  }

  socklen_t len = sizeof(*addr);
  if (getsockname(fd, (struct sockaddr*)addr, &len) != 0) {
    perror("getsockname");
    return 1;
  }

  return 0;
}

static int open_connected_udp_pair(int* sender, int* receiver) {
  *receiver = socket(AF_INET, SOCK_DGRAM, 0);
  if (*receiver < 0) {
    perror("socket(receiver)");
    return 1;
  }

  *sender = socket(AF_INET, SOCK_DGRAM, 0);
  if (*sender < 0) {
    perror("socket(sender)");
    close(*receiver);
    return 1;
  }

  struct sockaddr_in receiver_addr;
  struct sockaddr_in sender_addr;
  if (bind_loopback_udp(*receiver, &receiver_addr) != 0 ||
      bind_loopback_udp(*sender, &sender_addr) != 0) {
    close(*sender);
    close(*receiver);
    return 1;
  }

  if (connect(*sender, (struct sockaddr*)&receiver_addr,
              sizeof(receiver_addr)) != 0) {
    perror("connect(sender)");
    close(*sender);
    close(*receiver);
    return 1;
  }

  if (connect(*receiver, (struct sockaddr*)&sender_addr, sizeof(sender_addr)) !=
      0) {
    perror("connect(receiver)");
    close(*sender);
    close(*receiver);
    return 1;
  }

  return 0;
}

static int assert_poll_timeout(const char* label, int timeout_ms,
                               long min_elapsed_ms, long max_elapsed_ms) {
  int sender;
  int receiver;
  if (open_connected_udp_pair(&sender, &receiver) != 0) {
    return 1;
  }

  struct timespec before;
  struct timespec after;
  if (clock_gettime(CLOCK_MONOTONIC, &before) != 0) {
    perror("clock_gettime(before)");
    close(sender);
    close(receiver);
    return 1;
  }

  struct pollfd pfd = {.fd = receiver, .events = POLLIN, .revents = 0};
  int ready = poll(&pfd, 1, timeout_ms);

  if (clock_gettime(CLOCK_MONOTONIC, &after) != 0) {
    perror("clock_gettime(after)");
    close(sender);
    close(receiver);
    return 1;
  }

  long duration_ms = elapsed_ms(before, after);
  close(sender);
  close(receiver);

  if (ready != 0) {
    fprintf(stderr, "%s: expected poll timeout, got ready=%d revents=0x%x\n",
            label, ready, pfd.revents);
    return 1;
  }

  if (pfd.revents != 0) {
    fprintf(stderr, "%s: expected no revents, got 0x%x\n", label, pfd.revents);
    return 1;
  }

  if (duration_ms < min_elapsed_ms) {
    fprintf(stderr, "%s: poll returned too quickly: %ldms < %ldms\n", label,
            duration_ms, min_elapsed_ms);
    return 1;
  }

  if (duration_ms >= max_elapsed_ms) {
    fprintf(stderr, "%s: poll took too long: %ldms >= %ldms\n", label,
            duration_ms, max_elapsed_ms);
    return 1;
  }

  return 0;
}

int main(void) {
  if (assert_poll_timeout("zero timeout", 0, 0, 1000) != 0) {
    return 1;
  }

  if (assert_poll_timeout("two second timeout", 2000, 100, 3000) != 0) {
    return 1;
  }

  puts("UDP socket poll timeouts behaved as expected");
  return 0;
}
