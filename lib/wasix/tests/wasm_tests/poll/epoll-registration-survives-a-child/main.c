//#BuildEnv: WASIXCC_WASM_EXCEPTIONS=no
//#ExpectedStdout: 0
// A forked child holds copies of the parent's descriptors. Its closing them, or
// exiting, closes only those copies: the parent's registrations must survive,
// as they do on Linux, or a parent event loop that forks a helper stops seeing
// events.
#include <stdlib.h>
#include <sys/wait.h>
#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/epoll.h>
#include <sys/eventfd.h>
#include <unistd.h>

// wasix-libc's epoll_ctl hands back the errno itself on failure, rather than -1
// with errno set, so only "is it zero" is portable here.
static int ctl(int ep, int op, int fd, uint32_t events) {
  struct epoll_event event = {.events = events, .data.fd = fd};
  int rc = epoll_ctl(ep, op, fd, &event);
  return rc == -1 ? errno : rc;
}

static int ready(int ep, int timeout_ms) {
  struct epoll_event events[4];
  return epoll_wait(ep, events, 4, timeout_ms);
}

static void signal_event(int fd) {
  uint64_t one = 1;
  assert(write(fd, &one, sizeof(one)) == sizeof(one));
}

static void run_child(int ep, int efd, int close_first) {
  pid_t pid = fork();
  assert(pid >= 0);
  if (pid == 0) {
    if (close_first) {
      close(efd);
      close(ep);
    }
    _exit(0);
  }
  int status = 0;
  assert(waitpid(pid, &status, 0) == pid);
  assert(WIFEXITED(status) && WEXITSTATUS(status) == 0);
}

int main(void) {
  int ep = epoll_create1(0);
  int efd = eventfd(0, 0);
  assert(ep >= 0 && efd >= 0);
  assert(ctl(ep, EPOLL_CTL_ADD, efd, EPOLLIN) == 0);

  for (int close_first = 0; close_first < 2; close_first++) {
    run_child(ep, efd, close_first);
    signal_event(efd);
    assert(ready(ep, 1000) == 1);
    uint64_t drained = 0;
    assert(read(efd, &drained, sizeof(drained)) == sizeof(drained));
  }

  printf("0");
  return 0;
}
