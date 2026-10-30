#define _GNU_SOURCE
#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static int tasks(void) {
  DIR *proc = opendir("/proc");
  assert(proc);
  int count = 0;
  struct dirent *p;
  while ((p = readdir(proc))) {
    if (p->d_name[0] < '0' || p->d_name[0] > '9') continue;
    char path[128];
    snprintf(path, sizeof(path), "/proc/%s/task", p->d_name);
    DIR *threads = opendir(path);
    if (!threads) continue;
    struct dirent *t;
    while ((t = readdir(threads)))
      if (t->d_name[0] >= '0' && t->d_name[0] <= '9') count++;
    closedir(threads);
  }
  closedir(proc);
  return count;
}

static void reaped(pid_t pid, int sig) {
  int status;
  pid_t got;
  do { got = waitpid(pid, &status, 0); } while (got < 0 && errno == EINTR);
  assert(got == pid);
  if (sig) assert(WIFSIGNALED(status) && WTERMSIG(status) == sig);
  else assert(WIFEXITED(status) && WEXITSTATUS(status) == 0);
}

int main(int argc, char **argv) {
  assert(argc >= 2);
  if (!strcmp(argv[1], "tasks")) {
    printf("%d\n", tasks());
    return 0;
  }
  if (!strcmp(argv[1], "monitor")) {
    for (;;) {
      printf("TASKS %d\n", tasks());
      fflush(stdout);
      sleep(60);
    }
  }
  if (!strcmp(argv[1], "storm")) {
    unsigned seed = argc > 2 ? strtoul(argv[2], NULL, 10) : 1;
    srand(seed);
    for (int batch = 0; batch < 4; batch++) {
      pid_t children[4];
      for (int i = 0; i < 4; i++) {
        children[i] = fork();
        assert(children[i] >= 0);
        if (!children[i]) {
          execl("/bin/true", "true", (char *)NULL);
          _exit(127);
        }
      }
      for (int i = 0; i < 4; i++) reaped(children[i], 0);
    }
    /* Linux kernel/signal.c:do_signal_stop and kernel/exit.c:do_wait:
       observe each stop/continue before terminating and reaping the worker. */
    for (int i = 0; i < 4; i++) {
      int ready[2];
      assert(pipe(ready) == 0);
      pid_t pid = fork();
      assert(pid >= 0);
      if (!pid) {
        close(ready[0]);
        char byte = 1;
        assert(write(ready[1], &byte, 1) == 1);
        close(ready[1]);
        for (;;) pause();
      }
      close(ready[1]);
      char byte;
      assert(read(ready[0], &byte, 1) == 1);
      close(ready[0]);
      assert(kill(pid, SIGSTOP) == 0);
      int status;
      assert(waitpid(pid, &status, WUNTRACED) == pid);
      assert(WIFSTOPPED(status) && WSTOPSIG(status) == SIGSTOP);
      assert(kill(pid, SIGCONT) == 0);
      assert(waitpid(pid, &status, WCONTINUED) == pid && WIFCONTINUED(status));
      int sig = rand() & 1 ? SIGKILL : SIGTERM;
      assert(kill(pid, sig) == 0);
      reaped(pid, sig);
    }
    puts("storm PASS");
    return 0;
  }
  return 2;
}
