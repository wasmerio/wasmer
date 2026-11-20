#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/epoll.h>
#include <unistd.h>

static int failures;

static void check(int condition, const char* message) {
  if (!condition) {
    fprintf(stderr, "%s\n", message);
    failures++;
  }
}

static void require(int condition, const char* message) {
  if (!condition) {
    perror(message);
    exit(EXIT_FAILURE);
  }
}

static void require_pthread(int error, const char* message) {
  if (error != 0) {
    fprintf(stderr, "%s: %s\n", message, strerror(error));
    exit(EXIT_FAILURE);
  }
}

static int add_reader(int epoll_fd, int reader) {
  struct epoll_event event = {.events = EPOLLIN, .data.fd = reader};
  return epoll_ctl(epoll_fd, EPOLL_CTL_ADD, reader, &event);
}

static int wait_for_hup(int epoll_fd, int reader, int timeout_ms) {
  struct epoll_event event = {0};
  int count = epoll_wait(epoll_fd, &event, 1, timeout_ms);
  return count == 1 && event.data.fd == reader &&
         (event.events & EPOLLHUP) != 0;
}

static void cloned_writer_defers_hup_until_final_close(void) {
  int fds[2];
  require(pipe(fds) == 0, "pipe failed for cloned-writer test");
  int duplicate = dup(fds[1]);
  require(duplicate >= 0, "dup failed for cloned-writer test");
  int epoll_fd = epoll_create1(0);
  require(epoll_fd >= 0, "epoll_create1 failed for cloned-writer test");
  require(add_reader(epoll_fd, fds[0]) == 0,
          "epoll_ctl failed for cloned-writer test");

  require(close(fds[1]) == 0, "first writer close failed");
  struct epoll_event event = {0};
  check(epoll_wait(epoll_fd, &event, 1, 25) == 0,
        "non-final writer close reported HUP");

  require(close(duplicate) == 0, "final writer close failed");
  check(wait_for_hup(epoll_fd, fds[0], 1000),
        "final writer close did not report HUP");
  char byte;
  check(read(fds[0], &byte, sizeof(byte)) == 0,
        "final writer close did not expose EOF");

  close(fds[0]);
  close(epoll_fd);
}

static void late_registration_observes_hup(void) {
  int fds[2];
  require(pipe(fds) == 0, "pipe failed for late-registration test");
  require(close(fds[1]) == 0, "writer close failed before registration");
  int epoll_fd = epoll_create1(0);
  require(epoll_fd >= 0, "epoll_create1 failed for late-registration test");
  require(add_reader(epoll_fd, fds[0]) == 0,
          "late epoll_ctl registration failed");

  check(wait_for_hup(epoll_fd, fds[0], 1000),
        "late registration did not report HUP");

  close(fds[0]);
  close(epoll_fd);
}

static void buffered_payload_precedes_eof(void) {
  static const char payload[] = "payload";
  int fds[2];
  require(pipe(fds) == 0, "pipe failed for buffered-payload test");
  require(write(fds[1], payload, sizeof(payload)) == (ssize_t)sizeof(payload),
          "buffered payload write failed");
  require(close(fds[1]) == 0, "buffered writer close failed");
  int epoll_fd = epoll_create1(0);
  require(epoll_fd >= 0, "epoll_create1 failed for buffered-payload test");
  require(add_reader(epoll_fd, fds[0]) == 0,
          "epoll_ctl failed for buffered-payload test");

  check(wait_for_hup(epoll_fd, fds[0], 1000),
        "buffered final close did not report HUP");
  char buffer[sizeof(payload)] = {0};
  check(read(fds[0], buffer, sizeof(buffer)) == (ssize_t)sizeof(buffer),
        "buffered read returned the wrong length");
  check(memcmp(buffer, payload, sizeof(payload)) == 0,
        "buffered read returned the wrong payload");
  check(read(fds[0], buffer, sizeof(buffer)) == 0,
        "buffered pipe did not return EOF after its payload");

  close(fds[0]);
  close(epoll_fd);
}

struct waiter_args {
  int epoll_fd;
  int reader;
  atomic_bool ready;
  int saw_hup;
};

static void* wait_for_writer_close(void* raw_args) {
  struct waiter_args* args = raw_args;
  atomic_store(&args->ready, true);
  args->saw_hup = wait_for_hup(args->epoll_fd, args->reader, 1000);
  return NULL;
}

static void blocked_pthread_is_woken_by_final_close(void) {
  int fds[2];
  require(pipe(fds) == 0, "pipe failed for pthread test");
  int epoll_fd = epoll_create1(0);
  require(epoll_fd >= 0, "epoll_create1 failed for pthread test");
  require(add_reader(epoll_fd, fds[0]) == 0,
          "epoll_ctl failed for pthread test");

  struct waiter_args args = {
      .epoll_fd = epoll_fd, .reader = fds[0], .saw_hup = 0};
  atomic_init(&args.ready, false);
  pthread_t waiter;
  require_pthread(pthread_create(&waiter, NULL, wait_for_writer_close, &args),
                  "pthread_create failed");
  while (!atomic_load(&args.ready)) {
    sched_yield();
  }
  usleep(25000);
  require(close(fds[1]) == 0, "writer close failed for pthread test");
  require_pthread(pthread_join(waiter, NULL), "pthread_join failed");
  check(args.saw_hup, "blocked pthread did not wake with HUP");

  close(fds[0]);
  close(epoll_fd);
}

int main(void) {
  cloned_writer_defers_hup_until_final_close();
  late_registration_observes_hup();
  buffered_payload_precedes_eof();
  blocked_pthread_is_woken_by_final_close();
  return failures == 0 ? EXIT_SUCCESS : EXIT_FAILURE;
}
