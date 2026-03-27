/*
Tests:
- Setitimer creates the correct timer with the given value + interval.
- The signal handler gets the correct signal.
- Reseting the timer returns the time until the next signal.
- Setting the old timer pointer to `NULL` do not cause any issues.
- After the `value` timer is hit, the timer switches to the provided `interval`.
*/

#include <stdio.h>
#include <signal.h>
#include <time.h>
#include <sys/time.h>
#include <unistd.h>

#define NS_PER_SEC 1000000000LL
#define NS_PER_MS  1000000LL
#define US_PER_MS  1000LL

// 20 ms precision
#define TIME_DIFF_PRECISION 20 * NS_PER_MS 

#define VALUE_SEC 2
#define VALUE_MS  500

#define INTERVAL_SEC 1
#define INTERVAL_MS  200

static struct timespec armed_at;

long long ts_to_ns(struct timespec t) {
    return (long long)t.tv_sec * 1000000000LL + t.tv_nsec;
}

void handler(int signum) {    
    static int count = 0;
    struct timespec fired_at;
    long long base_diff;

    // Make sure the given signal is the correct one
    if (signum != SIGALRM) {
        _exit(1);
    }

    count++;

    clock_gettime(CLOCK_MONOTONIC, &fired_at);
    long long diff = ts_to_ns(fired_at) - ts_to_ns(armed_at);

    armed_at.tv_sec = fired_at.tv_sec;
    armed_at.tv_nsec = fired_at.tv_nsec;

    // If we hit here the first time, we check `it_value`, otherwise we check `it_interval`
    if (count == 1) {
        base_diff = VALUE_SEC * NS_PER_SEC + VALUE_MS * NS_PER_MS;
    } else {
        base_diff = INTERVAL_SEC * NS_PER_SEC + INTERVAL_MS * NS_PER_MS;
    }

    // Check if the time difference is within bounds
    if (diff > (base_diff + TIME_DIFF_PRECISION) || diff < (base_diff - TIME_DIFF_PRECISION)) {
        fprintf(stderr, "the time difference is invalid %lld %lld", diff, base_diff);
        _exit(1);
    }

    if (count == 3) {
        _exit(0);
    };
}

int main() {
    struct sigaction sa;
    struct itimerval timer;
    struct itimerval old_timer;

    // setup the signal handler for SIGALRM
    sa.sa_handler = handler;
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = 0;
    sigaction(SIGALRM, &sa, NULL);

    timer.it_value.tv_sec = 5;
    timer.it_value.tv_usec = 0;

    timer.it_interval.tv_sec = 2;
    timer.it_interval.tv_usec = 0;

    if (setitimer(ITIMER_REAL, &timer, NULL) == -1) {
        fprintf(stderr, "setitimer");
        return 1;
    }

    // The initial time interval is set to 2.5 seconds
    timer.it_value.tv_sec = VALUE_SEC;
    timer.it_value.tv_usec = VALUE_MS * US_PER_MS;

    // Later, we expect the timer to be set to 1.2 seconds
    timer.it_interval.tv_sec = INTERVAL_SEC;
    timer.it_interval.tv_usec = INTERVAL_MS * US_PER_MS;

    // We save the time at when we call `setitimer` to later compute the rough time spent until the sighandler
    // is executed.
    clock_gettime(CLOCK_MONOTONIC, &armed_at);

    if (setitimer(ITIMER_REAL, &timer, &old_timer) == -1) {
        fprintf(stderr, "setitimer");
        return 1;
    }

    // We initially set the timer to 5 seconds, and then immediately call `setitimer` again.
    // This should result in getting some value back like 4.999... seconds.
    if (old_timer.it_value.tv_sec != 4 || old_timer.it_value.tv_usec > 900 * NS_PER_MS) {
        fprintf(stderr, "the timer changed unexpectedly (%lld, %lld)", old_timer.it_value.tv_sec, old_timer.it_value.tv_usec);
        return 1;
    }

    struct timespec ts;
    ts.tv_sec = 0;
    ts.tv_nsec = 1 * NS_PER_MS; // 1ms
    int i = 100000; // 10 seconds wait total before failure
    while (i--) {
        // We are sleeping for 1ms to make the Wasmer signal handler run every 1ms
        // so that we can compute the time difference between the timer set
        // and the sighandler call accurately.
        nanosleep(&ts, NULL);
    }

    return 1;
}
