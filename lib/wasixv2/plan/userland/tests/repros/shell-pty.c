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
static const char *expect(const char *text) {
    static char output[32768];
    memset(output, 0, sizeof(output));
    size_t used = 0;
    for (int n = 0; n < 150; n++) {
        struct pollfd p = { .fd = master, .events = POLLIN };
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
static void send_line(const char *s) {
    size_t n = strlen(s);
    if (write(master, s, n) != (ssize_t)n) { perror("pty write"); exit(1); }
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    master = posix_openpt(O_RDWR | O_NOCTTY);
    if (master < 0 || grantpt(master) || unlockpt(master)) { perror("ptmx"); return 1; }
    char *name = ptsname(master);
    if (!name) { perror("ptsname"); return 1; }
    pid_t child = fork();
    if (child < 0) { perror("fork"); return 1; }
    if (!child) {
        if (setsid() < 0) _exit(10);
        int slave = open(name, O_RDWR);
        if (slave < 0 || ioctl(slave, TIOCSCTTY, 0) < 0) _exit(11);
        for (int i = 0; i < 3; i++) if (dup2(slave, i) < 0) _exit(12);
        if (slave > 2) close(slave);
        close(master);
        setenv("PS1", "V9> ", 1);
        setenv("ENV", "/dev/null", 1);
        setenv("TERM", "dumb", 1);
        execl(argv[1], argv[1], "-i", (char *)NULL);
        _exit(13);
    }
    expect("V9> ");
    send_line("sleep 30\n");
    usleep(200000);
    send_line("\032");
    expect("V9> ");
    send_line("jobs\n");
    if (!strstr(expect("V9> "), "Stopped")) {
        fputs("jobs did not report stopped child\n", stderr); return 1;
    }
    send_line("bg\n");
    expect("V9> ");
    send_line("kill %1; wait; printf 'job-control-%s\\n' ok\n");
    expect("job-control-ok");
    send_line("exit\n");
    int status;
    if (waitpid(child, &status, 0) != child) { perror("waitpid"); return 1; }
    close(master);
    if (!WIFEXITED(status) || WEXITSTATUS(status)) return 1;
    puts("pty job control PASS");
    return 0;
}
