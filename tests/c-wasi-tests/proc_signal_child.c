#include <assert.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

static volatile sig_atomic_t got_signal = 0;

static void handler(int sig)
{
    (void)sig;
    got_signal = 1;
}

static int parse_int(const char *value)
{
    char *end = NULL;
    long parsed = strtol(value, &end, 10);
    if (end == value) {
        return 0;
    }
    if (parsed < 0) {
        return 0;
    }
    if (parsed > 255) {
        return 255;
    }
    return (int)parsed;
}

int main(int argc, char **argv)
{
    int exit_code = 0;
    int timeout_ms = 1000;

    for (int i = 1; i < argc; ++i) {
        const char *arg = argv[i];
        if (strncmp(arg, "exit=", 5) == 0) {
            exit_code = parse_int(arg + 5);
            continue;
        }
        if (strncmp(arg, "timeout=", 8) == 0) {
            timeout_ms = parse_int(arg + 8);
            continue;
        }
    }

    void (*prev)(int) = signal(SIGUSR1, handler);
    assert(prev != SIG_ERR);

    int waited = 0;
    while (!got_signal && waited < timeout_ms) {
        usleep(1000);
        waited += 1;
    }

    if (!got_signal) {
        return 2;
    }

    __wasi_proc_exit((__wasi_exitcode_t)exit_code);
    assert(0 && "proc_exit returned");
    return 1;
}
