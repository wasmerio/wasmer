//#ExpectedStdout: builtin wasmer version passed

#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char** environ;

static void fail(const char* message) {
  perror(message);
  exit(1);
}

static size_t read_all(int fd, char* buffer, size_t capacity) {
  size_t length = 0;
  while (length < capacity) {
    ssize_t count = read(fd, buffer + length, capacity - length);
    if (count < 0) {
      fail("read");
    }
    if (count == 0) {
      break;
    }
    length += count;
  }
  return length;
}

int main(void) {
  int stdout_pipe[2];
  int stderr_pipe[2];
  if (pipe(stdout_pipe) != 0 || pipe(stderr_pipe) != 0) {
    fail("pipe");
  }

  posix_spawn_file_actions_t actions;
  if (posix_spawn_file_actions_init(&actions) != 0 ||
      posix_spawn_file_actions_adddup2(&actions, stdout_pipe[1],
                                       STDOUT_FILENO) != 0 ||
      posix_spawn_file_actions_adddup2(&actions, stderr_pipe[1],
                                       STDERR_FILENO) != 0 ||
      posix_spawn_file_actions_addclose(&actions, stdout_pipe[0]) != 0 ||
      posix_spawn_file_actions_addclose(&actions, stdout_pipe[1]) != 0 ||
      posix_spawn_file_actions_addclose(&actions, stderr_pipe[0]) != 0 ||
      posix_spawn_file_actions_addclose(&actions, stderr_pipe[1]) != 0) {
    return 1;
  }

  pid_t pid;
  char* argv[] = {"wasmer", "--version", NULL};
  int spawn_error = posix_spawnp(&pid, "wasmer", &actions, NULL, argv, environ);
  posix_spawn_file_actions_destroy(&actions);
  if (spawn_error != 0) {
    return spawn_error;
  }

  close(stdout_pipe[1]);
  close(stderr_pipe[1]);
  char stdout_output[128] = {0};
  char stderr_output[128] = {0};
  size_t stdout_length =
      read_all(stdout_pipe[0], stdout_output, sizeof(stdout_output) - 1);
  size_t stderr_length =
      read_all(stderr_pipe[0], stderr_output, sizeof(stderr_output) - 1);
  close(stdout_pipe[0]);
  close(stderr_pipe[0]);

  int status;
  if (waitpid(pid, &status, 0) != pid) {
    fail("waitpid");
  }

  if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
    fprintf(stderr, "wasmer exited with status %d\n", status);
    return 1;
  }

  if (stdout_length == 0 ||
      strncmp(stdout_output, "wasmer ", strlen("wasmer ")) != 0) {
    fprintf(stderr, "unexpected wasmer stdout: %s\n", stdout_output);
    return 1;
  }
  if (stderr_length != 0) {
    fprintf(stderr, "unexpected wasmer stderr: %s\n", stderr_output);
    return 1;
  }

  puts("builtin wasmer version passed");
  return 0;
}
