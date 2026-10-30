#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/prctl.h>
#include <sys/wait.h>
#include <unistd.h>

static void* worker(void* unused) {
  (void)unused;
  pthread_setname_np(pthread_self(), "v15-worker");
  for (;;) usleep(100000);
  return NULL;
}
int main(int argc, char** argv) {
  if (argc != 2) return 2;
  const int vim = !strcmp(argv[1], "vim");
  int master = posix_openpt(O_RDWR | O_NOCTTY);
  if (master < 0 || grantpt(master) || unlockpt(master)) return 3;
  char slave_name[128];
  snprintf(slave_name, sizeof(slave_name), "%s", ptsname(master));
  struct winsize size = {.ws_row = 30, .ws_col = 120};
  if (ioctl(master, TIOCSWINSZ, &size)) return 4;
  pid_t child = fork();
  if (child < 0) return 5;
  if (!child) {
    if (setsid() < 0) _exit(10);
    int slave = open(slave_name, O_RDWR);
    if (slave < 0 || ioctl(slave, TIOCSCTTY, 0)) _exit(11);
    for (int i = 0; i < 3; ++i)
      if (dup2(slave, i) < 0) _exit(12);
    if (slave > 2) close(slave);
    close(master);
    setenv("TERM", "xterm", 1);
    setenv("TERMINFO", "/terminfo", 1);
    setenv("HOME", "/tmp", 1);
    if (vim)
      execl("/bin/vim", "vim", "-Nu", "NONE", "-n", "/work/vim-result",
            (char*)NULL);
    else
      execl("/bin/htop", "htop", "--delay=1", "--filter=v15-", (char*)NULL);
    _exit(13);
  }
  pthread_t thread;
  if (!vim) {
    prctl(PR_SET_NAME, "v15-driver");
    if (pthread_create(&thread, NULL, worker, NULL)) return 6;
  }
  FILE* out = fopen(vim ? "/work/vim.pty" : "/work/htop.pty", "w");
  if (!out) return 7;
  char captured[262144] = {0};
  size_t used = 0;
  int status = 0, ended = 0;
  for (int tick = 0; tick < 200; ++tick) {
    struct pollfd p = {.fd = master, .events = POLLIN};
    int ready = poll(&p, 1, 100);
    if (ready > 0 && (p.revents & POLLIN)) {
      char buffer[8192];
      ssize_t len = read(master, buffer, sizeof(buffer));
      if (len > 0) {
        fwrite(buffer, 1, len, out);
        if (used + (size_t)len < sizeof(captured)) {
          memcpy(captured + used, buffer, len);
          used += len;
        }
      }
    }
    if (tick == 15) {
      const char* keys = vim ? "iV15 edit-save verified\n\033:wq\n" : "H";
      if (write(master, keys, strlen(keys)) != (ssize_t)strlen(keys)) return 8;
    }
    if (!vim && tick == 45 && write(master, "q", 1) != 1) return 9;
    if (waitpid(child, &status, WNOHANG) == child) {
      ended = 1;
      break;
    }
  }
  fclose(out);
  if (!ended) {
    kill(child, SIGKILL);
    waitpid(child, &status, 0);
  }
  close(master);
  if (!ended || !WIFEXITED(status) || WEXITSTATUS(status)) {
    fprintf(stderr, "%s status=%d ended=%d\n", argv[1], status, ended);
    return 20;
  }
  if (vim) {
    char line[128] = {0};
    FILE* saved = fopen("/work/vim-result", "r");
    if (!saved || !fgets(line, sizeof(line), saved) ||
        strcmp(line, "V15 edit-save verified\n"))
      return 21;
    fclose(saved);
  } else {
    if (!strstr(captured, "Mem") || !strstr(captured, "Tasks") ||
        !strstr(captured, "v15-")) {
      fputs("htop missing memory/task/process display\n", stderr);
      return 22;
    }
    FILE* proc = fopen("/proc/self/status", "r");
    char line[256];
    int threads = 0;
    while (proc && fgets(line, sizeof(line), proc))
      sscanf(line, "Threads: %d", &threads);
    if (proc) fclose(proc);
    if (threads != 2) return 23;
  }
  printf("%s guest PTY PASS\n", argv[1]);
  return 0;
}
