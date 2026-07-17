#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static int wait_word = 0;

static int expect_atomic_wait_timeout(const char* name) {
  int result = __builtin_wasm_memory_atomic_wait32(&wait_word, 0, 1000000LL);
  if (result != 2) {
    printf("%s expected atomic wait timeout, got %d\n", name, result);
    return EXIT_FAILURE;
  }
  return EXIT_SUCCESS;
}

static void wait_forever(const char* name) {
  printf("%s waiting\n", name);
  fflush(stdout);

  __builtin_wasm_memory_atomic_wait32(&wait_word, 0, -1LL);

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

  if (expect_atomic_wait_timeout("targeted parent") != EXIT_SUCCESS) {
    return EXIT_FAILURE;
  }

  puts("targeted parent survived");
  return EXIT_SUCCESS;
}

static void* kill_current_process(void* arg) {
  pid_t pid = *(pid_t*)arg;
  usleep(100 * 1000);
  if (kill(pid, SIGKILL) != 0) {
    perror("kill");
  }
  return NULL;
}

static int vfork_child(void) {
  pid_t child = vfork();
  if (child < 0) {
    perror("vfork");
    return EXIT_FAILURE;
  }

  if (child == 0) {
    pid_t child_pid = getpid();
    pthread_t signaler;
    if (pthread_create(&signaler, NULL, kill_current_process, &child_pid) !=
        0) {
      perror("pthread_create");
      _Exit(EXIT_FAILURE);
    }

    puts("vfork child waiting");
    fflush(stdout);

    __builtin_wasm_memory_atomic_wait32(&wait_word, 0, -1LL);
    sleep(1);
    _Exit(42);
  }

  if (wait_for_children(1) != EXIT_SUCCESS) {
    return EXIT_FAILURE;
  }

  puts("vfork parent survived");
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

int main(int argc, char** argv) {
  if (argc != 2) {
    return EXIT_FAILURE;
  }

  if (strcmp(argv[1], "targeted") == 0) {
    return targeted_child();
  }

  if (strcmp(argv[1], "forwarded") == 0) {
    return forwarded_to_children();
  }

  if (strcmp(argv[1], "vfork") == 0) {
    return vfork_child();
  }

  return EXIT_FAILURE;
}
