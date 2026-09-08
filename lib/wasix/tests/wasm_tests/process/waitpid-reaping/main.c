//#BuildEnv: WASIXCC_WASM_EXCEPTIONS=no

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

struct blocked_child {
  pid_t pid;
  int release_fd;
};

static int failures;

static void fail(const char* message) {
  perror(message);
  exit(EXIT_FAILURE);
}

static void check(int condition, const char* message) {
  if (!condition) {
    fprintf(stderr, "%s\n", message);
    failures++;
  }
}

static void write_byte(int fd) {
  const char byte = 'x';
  ssize_t count;
  do {
    count = write(fd, &byte, sizeof(byte));
  } while (count < 0 && errno == EINTR);
  if (count != sizeof(byte)) {
    fail("write");
  }
}

static void read_byte(int fd) {
  char byte;
  ssize_t count;
  do {
    count = read(fd, &byte, sizeof(byte));
  } while (count < 0 && errno == EINTR);
  if (count != sizeof(byte)) {
    fail("read");
  }
}

static struct blocked_child spawn_blocked_child(int exit_code) {
  int ready[2];
  int release[2];
  if (pipe(ready) != 0 || pipe(release) != 0) {
    fail("pipe");
  }

  pid_t pid = fork();
  if (pid < 0) {
    fail("fork");
  }
  if (pid == 0) {
    close(ready[0]);
    close(release[1]);
    write_byte(ready[1]);
    close(ready[1]);
    read_byte(release[0]);
    close(release[0]);
    _Exit(exit_code);
  }

  close(ready[1]);
  close(release[0]);
  read_byte(ready[0]);
  close(ready[0]);
  return (struct blocked_child){.pid = pid, .release_fd = release[1]};
}

static int exited_with(int status, int exit_code) {
  return WIFEXITED(status) && WEXITSTATUS(status) == exit_code;
}

static void release_child(struct blocked_child child) {
  write_byte(child.release_fd);
  close(child.release_fd);
}

static void wait_for_child(pid_t pid, int exit_code) {
  int status = 0;
  check(waitpid(pid, &status, 0) == pid, "blocking wait returned wrong pid");
  check(exited_with(status, exit_code), "blocking wait returned wrong status");
}

static void expect_echild(pid_t pid, const char* message) {
  int status = 0;
  errno = 0;
  pid_t result = waitpid(pid, &status, WNOHANG);
  check(result == -1 && errno == ECHILD, message);
}

static void specific_nonblocking_reaps_once(void) {
  struct blocked_child child = spawn_blocked_child(37);
  int status = 0;

  check(waitpid(child.pid, &status, WNOHANG) == 0,
        "first targeted WNOHANG did not return zero");
  check(waitpid(child.pid, &status, WNOHANG) == 0,
        "second targeted WNOHANG did not return zero");

  release_child(child);
  wait_for_child(child.pid, 37);
  expect_echild(child.pid, "repeated targeted wait did not return ECHILD");
}

static void any_nonblocking_returns_promptly(void) {
  struct blocked_child child = spawn_blocked_child(38);
  int status = 0;

  check(waitpid(-1, &status, WNOHANG) == 0,
        "first any-child WNOHANG did not return zero");
  check(waitpid(-1, &status, WNOHANG) == 0,
        "second any-child WNOHANG did not return zero");
  release_child(child);
  wait_for_child(child.pid, 38);
  expect_echild(-1,
                "wait after the only child was reaped did not return ECHILD");
}

static void nonchild_is_not_waitable(void) {
  struct blocked_child child = spawn_blocked_child(39);
  expect_echild(getpid(), "waitpid accepted a process that was not a child");
  release_child(child);
  wait_for_child(child.pid, 39);
}

static void two_children_are_reported_once(void) {
  struct blocked_child first = spawn_blocked_child(41);
  struct blocked_child second = spawn_blocked_child(42);
  release_child(first);
  release_child(second);

  int first_status = 0;
  int second_status = 0;
  pid_t first_result = waitpid(-1, &first_status, 0);
  pid_t second_result = waitpid(-1, &second_status, 0);
  check(first_result > 0 && second_result > 0 && first_result != second_result,
        "any-child waits did not report two distinct children");

  int first_ok = first_result == first.pid && exited_with(first_status, 41);
  int second_ok = first_result == second.pid && exited_with(first_status, 42);
  check(first_ok || second_ok,
        "first any-child wait returned the wrong status");
  first_ok = second_result == first.pid && exited_with(second_status, 41);
  second_ok = second_result == second.pid && exited_with(second_status, 42);
  check(first_ok || second_ok,
        "second any-child wait returned the wrong status");
  expect_echild(-1, "third any-child wait did not return ECHILD");
}

static void completed_nonblocking_child_is_reaped(void) {
  struct blocked_child child = spawn_blocked_child(43);
  release_child(child);
  int status = 0;
  pid_t result = 0;

  for (int attempt = 0; attempt < 5000 && result == 0; attempt++) {
    result = waitpid(child.pid, &status, WNOHANG);
    if (result == 0) {
      usleep(1000);
    }
  }

  check(result == child.pid, "completed child was not returned by WNOHANG");
  check(exited_with(status, 43), "completed WNOHANG returned the wrong status");
  expect_echild(child.pid, "completed child was reported more than once");
}

int main(void) {
  specific_nonblocking_reaps_once();
  any_nonblocking_returns_promptly();
  nonchild_is_not_waitable();
  two_children_are_reported_once();
  completed_nonblocking_child_is_reaped();
  return failures == 0 ? EXIT_SUCCESS : EXIT_FAILURE;
}
