#include <errno.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static _Atomic uint32_t wait_word = 0;

static void wait_forever(const char *name) {
  printf("%s waiting\n", name);
  fflush(stdout);

  __builtin_wasm_memory_atomic_wait32((int *)&wait_word, 0, -1LL);

  printf("%s woken\n", name);
  fflush(stdout);

  sleep(1);
  _Exit(42);
}

static int wait_for_children(int count) {
  for (int i = 0; i < count; i++) {
    int status;
    pid_t pid;

    do {
      pid = wait(&status);
    } while (pid < 0 && errno == EINTR);

    if (pid < 0) {
      perror("wait");
      return EXIT_FAILURE;
    }
  }

  return EXIT_SUCCESS;
}

static int targeted_child(void) {
  pid_t child = fork();
  if (child < 0) {
    perror("fork");
    return EXIT_FAILURE;
  }

  if (child == 0) {
    wait_forever("targeted child");
  }

  usleep(100 * 1000);
  if (kill(child, SIGKILL) != 0) {
    perror("kill");
    return EXIT_FAILURE;
  }

  if (wait_for_children(1) != EXIT_SUCCESS) {
    return EXIT_FAILURE;
  }

  puts("targeted parent survived");
  return EXIT_SUCCESS;
}

static int forwarded_to_children(void) {
  pid_t parent = getpid();

  for (int i = 0; i < 2; i++) {
    pid_t child = fork();
    if (child < 0) {
      perror("fork");
      return EXIT_FAILURE;
    }

    if (child == 0) {
      wait_forever(i == 0 ? "forwarded child 1" : "forwarded child 2");
    }
  }

  pid_t signaler = fork();
  if (signaler < 0) {
    perror("fork");
    return EXIT_FAILURE;
  }

  if (signaler == 0) {
    usleep(100 * 1000);
    if (kill(parent, SIGTERM) != 0) {
      perror("kill");
      _Exit(EXIT_FAILURE);
    }
    _Exit(EXIT_SUCCESS);
  }

  puts("forwarding parent waiting");
  fflush(stdout);

  if (wait_for_children(3) != EXIT_SUCCESS) {
    return EXIT_FAILURE;
  }

  puts("forwarding parent survived");
  return EXIT_SUCCESS;
}

int main(int argc, char **argv) {
  if (argc != 2) {
    return EXIT_FAILURE;
  }

  if (strcmp(argv[1], "targeted") == 0) {
    return targeted_child();
  }

  if (strcmp(argv[1], "forwarded") == 0) {
    return forwarded_to_children();
  }

  return EXIT_FAILURE;
}
