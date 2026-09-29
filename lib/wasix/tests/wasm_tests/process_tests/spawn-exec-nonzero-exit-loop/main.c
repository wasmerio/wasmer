//#ExpectedStdout: spawn-exec-nonzero-exit-loop passed

#include <assert.h>
#include <spawn.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#ifndef ROUNDS
#error "ROUNDS must be defined by build.sh"
#endif

#ifndef CHILDREN_PER_ROUND
#error "CHILDREN_PER_ROUND must be defined by build.sh"
#endif

static void spawn_launcher(pid_t* pid) {
  posix_spawn_file_actions_t actions;
  posix_spawnattr_t attr;
  char* argv[] = {"main", "launcher", NULL};
  char* envp[] = {NULL};

  assert(posix_spawn_file_actions_init(&actions) == 0);
  assert(posix_spawnattr_init(&attr) == 0);
  assert(posix_spawn(pid, "./main", &actions, &attr, argv, envp) == 0);
  assert(posix_spawn_file_actions_destroy(&actions) == 0);
  assert(posix_spawnattr_destroy(&attr) == 0);
}

int main(int argc, char** argv) {
  if (argc == 2 && strcmp(argv[1], "leaf") == 0) {
    return 1;
  }

  if (argc == 2 && strcmp(argv[1], "launcher") == 0) {
    execl("./main.wasm", "main.wasm", "leaf", NULL);
    return 127;
  }

  for (int round = 0; round < ROUNDS; ++round) {
    pid_t pids[CHILDREN_PER_ROUND];

    for (int slot = 0; slot < CHILDREN_PER_ROUND; ++slot) {
      spawn_launcher(&pids[slot]);
    }

    for (int slot = 0; slot < CHILDREN_PER_ROUND; ++slot) {
      int status = 0;
      assert(waitpid(pids[slot], &status, 0) == pids[slot]);
      if (!WIFEXITED(status) || WEXITSTATUS(status) != 1) {
        printf("round %d slot %d: expected exit 1, got %d\n", round, slot,
               WIFEXITED(status) ? WEXITSTATUS(status) : -1);
        return 1;
      }
    }
  }

  puts("spawn-exec-nonzero-exit-loop passed");
  return 0;
}
