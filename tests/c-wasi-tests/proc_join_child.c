#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

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
    int sleep_ms = 0;

    for (int i = 1; i < argc; ++i) {
        const char *arg = argv[i];
        if (strncmp(arg, "sleep=", 6) == 0) {
            sleep_ms = parse_int(arg + 6);
            continue;
        }
        if (strncmp(arg, "exit=", 5) == 0) {
            exit_code = parse_int(arg + 5);
            continue;
        }
    }

    if (sleep_ms > 0) {
        usleep((useconds_t)sleep_ms * 1000u);
    }

    __wasi_proc_exit((__wasi_exitcode_t)exit_code);
    assert(0 && "proc_exit returned");
    return 1;
}
