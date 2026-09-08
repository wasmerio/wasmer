//#ExpectedStdout: child cwd spawn resolution passed

#include <errno.h>
#include <fcntl.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char** environ;

static int expect_parent_cwd(void) {
  char cwd[64];
  if (getcwd(cwd, sizeof(cwd)) == NULL) {
    perror("getcwd");
    return -1;
  }
  if (strcmp(cwd, "/home") != 0) {
    fprintf(stderr, "parent cwd changed to %s\n", cwd);
    return -1;
  }
  return 0;
}

static int spawn_and_expect(const char* name,
                            posix_spawn_file_actions_t* actions,
                            int search_path, int expected_exit) {
  pid_t pid = -1;
  char* argv[] = {(char*)name, NULL};
  int error = search_path
                  ? posix_spawnp(&pid, name, actions, NULL, argv, environ)
                  : posix_spawn(&pid, name, actions, NULL, argv, environ);
  if (error != 0) {
    fprintf(stderr, "spawn %s failed: %s\n", name, strerror(error));
    return -1;
  }

  int status;
  if (waitpid(pid, &status, 0) != pid) {
    perror("waitpid");
    return -1;
  }
  if (!WIFEXITED(status) || WEXITSTATUS(status) != expected_exit) {
    fprintf(stderr, "spawn %s: expected exit %d, got status %d\n", name,
            expected_exit, status);
    return -1;
  }
  return expect_parent_cwd();
}

static int expect_failed_spawn(const char* name,
                               posix_spawn_file_actions_t* actions,
                               int search_path, int expected_error) {
  pid_t pid = 12345;
  char* argv[] = {(char*)name, NULL};
  int error = search_path
                  ? posix_spawnp(&pid, name, actions, NULL, argv, environ)
                  : posix_spawn(&pid, name, actions, NULL, argv, environ);
  if (error != expected_error) {
    fprintf(stderr, "spawn %s: expected error %s, got %s\n", name,
            strerror(expected_error), strerror(error));
    return -1;
  }
  if (pid != 12345) {
    fprintf(stderr, "failed spawn %s published pid %d\n", name, (int)pid);
    return -1;
  }

  int status;
  errno = 0;
  if (waitpid(-1, &status, WNOHANG) != -1 || errno != ECHILD) {
    fprintf(stderr, "failed spawn %s left a waitable child\n", name);
    return -1;
  }
  return expect_parent_cwd();
}

int main(void) {
  posix_spawn_file_actions_t actions;

  if (posix_spawn_file_actions_init(&actions) != 0 ||
      posix_spawn_file_actions_addchdir_np(&actions, "chdir") != 0 ||
      spawn_and_expect("./tool", &actions, 0, 11) != 0 ||
      posix_spawn_file_actions_destroy(&actions) != 0)
    return EXIT_FAILURE;

  if (posix_spawn_file_actions_init(&actions) != 0 ||
      posix_spawn_file_actions_addchdir_np(&actions, "chdir") != 0 ||
      spawn_and_expect("sub/tool", &actions, 0, 12) != 0 ||
      posix_spawn_file_actions_destroy(&actions) != 0)
    return EXIT_FAILURE;

  int dirfd = open("fchdir", O_RDONLY | O_DIRECTORY);
  if (dirfd < 0 || posix_spawn_file_actions_init(&actions) != 0 ||
      posix_spawn_file_actions_addfchdir_np(&actions, dirfd) != 0 ||
      spawn_and_expect("tool", &actions, 0, 13) != 0 ||
      posix_spawn_file_actions_destroy(&actions) != 0)
    return EXIT_FAILURE;

  if (setenv("PATH", "bin:/missing", 1) != 0 ||
      posix_spawn_file_actions_init(&actions) != 0 ||
      posix_spawn_file_actions_addchdir_np(&actions, "path-relative") != 0 ||
      spawn_and_expect("tool", &actions, 1, 14) != 0 ||
      posix_spawn_file_actions_destroy(&actions) != 0)
    return EXIT_FAILURE;

  if (setenv("PATH", ":/missing", 1) != 0 ||
      posix_spawn_file_actions_init(&actions) != 0 ||
      posix_spawn_file_actions_addchdir_np(&actions, "path-empty") != 0 ||
      spawn_and_expect("tool", &actions, 1, 15) != 0 ||
      posix_spawn_file_actions_destroy(&actions) != 0)
    return EXIT_FAILURE;

  if (posix_spawn_file_actions_init(&actions) != 0 ||
      posix_spawn_file_actions_addfchdir_np(&actions, dirfd) != 0 ||
      posix_spawn_file_actions_addchdir_np(&actions, "nested") != 0 ||
      spawn_and_expect("./tool", &actions, 0, 16) != 0 ||
      posix_spawn_file_actions_destroy(&actions) != 0)
    return EXIT_FAILURE;

  if (posix_spawn_file_actions_init(&actions) != 0 ||
      posix_spawn_file_actions_addchdir_np(&actions, "missing") != 0 ||
      expect_failed_spawn("/home/tool", &actions, 0, ENOENT) != 0 ||
      posix_spawn_file_actions_destroy(&actions) != 0)
    return EXIT_FAILURE;

  if (setenv("PATH", "", 1) != 0 ||
      posix_spawn_file_actions_init(&actions) != 0 ||
      posix_spawn_file_actions_addchdir_np(&actions, "path-empty") != 0 ||
      expect_failed_spawn("missing", &actions, 1, ENOENT) != 0 ||
      posix_spawn_file_actions_destroy(&actions) != 0)
    return EXIT_FAILURE;

  close(dirfd);
  puts("child cwd spawn resolution passed");
  return EXIT_SUCCESS;
}
