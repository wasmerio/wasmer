//#ExpectedStdout: 0
// A descriptor number that one thread closed twice in a row must not be taken
// away from another thread that was handed the same number in between.
//
// Some guest toolchains close sockets twice when the owning object is dropped.
// In a multi-threaded program the second close can race with another thread
// re-using the (lowest free) number, and would then close that thread's brand
// new descriptor: "EBADF on a file I just opened", "EBADF from accept",
// "EEXIST from epoll_ctl". The runtime must answer such a stale duplicate close
// with EBADF and leave the other thread's descriptor alone, while still
// behaving normally for every legitimate reuse pattern.
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static int open_file(const char* name) {
  int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0644);
  assert(fd >= 0);
  return fd;
}

struct opener_args {
  const char* name;
  int fd;
};

static void* opener(void* arg) {
  struct opener_args* args = arg;
  args->fd = open_file(args->name);
  return NULL;
}

// Opens `name` on a freshly spawned thread and returns the descriptor it got.
static int open_on_other_thread(const char* name) {
  pthread_t thread;
  struct opener_args args = {.name = name, .fd = -1};
  assert(pthread_create(&thread, NULL, opener, &args) == 0);
  assert(pthread_join(thread, NULL) == 0);
  assert(args.fd >= 0);
  return args.fd;
}

static void test_stale_double_close_keeps_other_threads_fd(void) {
  int a = open_file("stale_close_a");
  assert(close(a) == 0);

  // Another thread gets the lowest free number, i.e. the one just closed.
  int b = open_on_other_thread("stale_close_b");
  assert(b == a);

  // The stale duplicate close of `a` must not close `b`.
  assert(close(a) == -1);
  assert(errno == EBADF);
  assert(write(b, "ok", 2) == 2);
  assert(close(b) == 0);
}

static void test_same_thread_reuse_closes_normally(void) {
  int a = open_file("stale_close_c");
  assert(close(a) == 0);
  int b = open_file("stale_close_d");
  assert(b == a);
  assert(close(b) == 0);
  assert(fcntl(b, F_GETFD) == -1);
  assert(errno == EBADF);
}

static void test_handed_over_fd_can_be_closed_after_use(void) {
  int a = open_file("stale_close_e");
  assert(close(a) == 0);
  int b = open_on_other_thread("stale_close_f");
  assert(b == a);
  // Using the number shows this thread legitimately owns it now.
  assert(write(b, "ok", 2) == 2);
  assert(close(b) == 0);
  assert(fcntl(b, F_GETFD) == -1);
  assert(errno == EBADF);
}

static void test_plain_double_close_reports_ebadf(void) {
  int a = open_file("stale_close_g");
  assert(close(a) == 0);
  assert(close(a) == -1);
  assert(errno == EBADF);
  int b = open_file("stale_close_h");
  assert(b == a);
  assert(write(b, "ok", 2) == 2);
  assert(close(b) == 0);
}

int main(void) {
  test_stale_double_close_keeps_other_threads_fd();
  test_same_thread_reuse_closes_normally();
  test_handed_over_fd_can_be_closed_after_use();
  test_plain_double_close_reports_ebadf();

  const char* files[] = {"stale_close_a", "stale_close_b", "stale_close_c",
                         "stale_close_d", "stale_close_e", "stale_close_f",
                         "stale_close_g", "stale_close_h"};
  for (size_t i = 0; i < sizeof(files) / sizeof(files[0]); i++) {
    unlink(files[i]);
  }

  printf("0");
  return 0;
}
