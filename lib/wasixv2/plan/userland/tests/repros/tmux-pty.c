#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <unistd.h>

static int master;
static const char* expect(const char* text) {
  static char output[32768];
  memset(output, 0, sizeof(output));
  size_t used = 0;
  for (int n = 0; n < 150; n++) {
    struct pollfd p = {.fd = master, .events = POLLIN};
    int r = poll(&p, 1, 100);
    if (r < 0 && errno == EINTR) continue;
    if (r <= 0) continue;
    ssize_t len = read(master, output + used, sizeof(output) - used - 1);
    if (len <= 0) break;
    fwrite(output + used, 1, len, stdout);
    fflush(stdout);
    used += len;
    if (strstr(output, text)) return output;
    if (used == sizeof(output) - 1) break;
  }
  fprintf(stderr, "pty: expected %s\n", text);
  exit(1);
}
static pid_t client(const char* slave_name, int attach) {
  pid_t pid = fork();
  if (pid < 0) {
    perror("fork");
    exit(1);
  }
  if (!pid) {
    if (setsid() < 0) _exit(10);
    int slave = open(slave_name, O_RDWR);
    if (slave < 0 || ioctl(slave, TIOCSCTTY, 0) < 0) _exit(11);
    for (int i = 0; i < 3; i++)
      if (dup2(slave, i) < 0) _exit(12);
    if (slave > 2) close(slave);
    close(master);
    setenv("TERM", "xterm", 1);
    setenv("TERMINFO", "/terminfo", 1);
    setenv("SHELL", "/bin/bash", 1);
    if (attach)
      execl("/bin/tmux", "tmux", "-L", "v11", "attach-session", "-t", "v11",
            (char*)NULL);
    else
      execl("/bin/tmux", "tmux", "-L", "v11", "-f", "/dev/null", "new-session",
            "-s", "v11", "/bin/bash", (char*)NULL);
    _exit(13);
  }
  return pid;
}
static void command(const char* text) {
  int status = system(text);
  if (status != 0) {
    fprintf(stderr, "command failed (%d): %s\n", status, text);
    exit(1);
  }
}
static void reaped(pid_t pid) {
  int status;
  if (waitpid(pid, &status, 0) != pid || !WIFEXITED(status) ||
      WEXITSTATUS(status)) {
    fputs("tmux client failed\n", stderr);
    exit(1);
  }
}
int main(void) {
  setenv("PATH", "/bin:/usr/bin", 1);
  master = posix_openpt(O_RDWR | O_NOCTTY);
  if (master < 0 || grantpt(master) || unlockpt(master)) {
    perror("ptmx");
    return 1;
  }
  char* name = ptsname(master);
  if (!name) {
    perror("ptsname");
    return 1;
  }
  char slave_name[128];
  snprintf(slave_name, sizeof(slave_name), "%s", name);
  struct winsize size = {.ws_row = 24, .ws_col = 80};
  if (ioctl(master, TIOCSWINSZ, &size)) {
    perror("winsize");
    return 1;
  }
  pid_t first = client(slave_name, 0);
  int ready = 0;
  for (int i = 0; i < 100; i++) {
    if (system("tmux -L v11 has-session -t v11 2>/dev/null") == 0) {
      ready = 1;
      break;
    }
    usleep(50000);
  }
  if (!ready) {
    fputs("tmux server did not start\n", stderr);
    return 1;
  }
  command("tmux -L v11 split-window -h -t v11 /bin/bash");
  command("tmux -L v11 send-keys -t v11:0.0 " printf 'tmux-%s\\n' first
          " Enter");
  expect("tmux-first");
  command("tmux -L v11 capture-pane -p -t v11:0.0 | grep -q tmux-first");
  command("tmux -L v11 detach-client -s v11");
  reaped(first);
  command("tmux -L v11 has-session -t v11");
  pid_t second = client(slave_name, 1);
  usleep(200000);
  command("tmux -L v11 send-keys -t v11:0.1 " printf 'tmux-%s\\n' attached
          " Enter");
  expect("tmux-attached");
  command("tmux -L v11 capture-pane -p -t v11:0.1 | grep -q tmux-attached");
  command("tmux -L v11 kill-server");
  reaped(second);
  close(master);
  puts("tmux pty split/send-keys/detach/attach PASS");
  return 0;
}
