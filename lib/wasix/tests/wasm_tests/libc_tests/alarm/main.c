/*
Tests:
- Alarm triggers the signal handler after the given time.
- Signal handler gets the correct signal after the `handler` hits.
- The signal is raised only once.
*/

#include <stdio.h>
#include <signal.h>
#include <time.h>
#include <unistd.h>

#define NS_PER_SEC 1000000000LL
#define NS_PER_MS  1000000LL

#define TIME_DIFF_PRECISION (20 * NS_PER_MS)

#define ALARM_SEC 1

static struct timespec armed_at;

static volatile sig_atomic_t signal_count = 0;
static volatile sig_atomic_t alarm_fired = 0;

long long ts_to_ns(struct timespec t) {
    return (long long)t.tv_sec * NS_PER_SEC + t.tv_nsec;
}

void handler(int signum) {
    struct timespec fired_at;
    long long base_diff;

    if (signum != SIGALRM) {
        _exit(1);
    }

    signal_count++;
    if (signal_count > 1) {
        _exit(1);
    }

    clock_gettime(CLOCK_MONOTONIC, &fired_at);
    long long diff = ts_to_ns(fired_at) - ts_to_ns(armed_at);

    base_diff = ALARM_SEC * NS_PER_SEC;

    // Check if the time difference is within bounds
    if (diff > (base_diff + TIME_DIFF_PRECISION) || diff < (base_diff - TIME_DIFF_PRECISION)) {
        fprintf(stderr, "the time difference is invalid %lld %lld", diff, base_diff);
        _exit(1);
    }

    alarm_fired = 1;
}

int main() {
    struct sigaction sa;

    // setup the signal handler for SIGALRM
    sa.sa_handler = handler;
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = 0;
    sigaction(SIGALRM, &sa, NULL);

    clock_gettime(CLOCK_MONOTONIC, &armed_at);
    if (alarm(ALARM_SEC) != 0) {
        fprintf(stderr, "alarm unexpectedly replaced an existing alarm");
        return 1;
    }

    struct timespec ts;
    ts.tv_sec = 0;
    ts.tv_nsec = 1 * NS_PER_MS; // 1ms

    int i = 3000; // 3 seconds total wait before failure/success
    while (i--) {
        nanosleep(&ts, NULL);
    }

    if (!alarm_fired) {
        fprintf(stderr, "alarm did not fire\n");
        return 1;
    }

    if (signal_count != 1) {
        fprintf(stderr, "handler ran unexpected number of times: %d\n", signal_count);
        return 1;
    }

    return 0;
}
