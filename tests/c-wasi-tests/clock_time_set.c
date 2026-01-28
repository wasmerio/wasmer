#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <time.h>

#define NSEC_PER_SEC INT64_C(1000000000)
#define ALLOWED_DRIFT_NS (5 * NSEC_PER_SEC)

static int64_t diff_ns(const struct timespec *a, const struct timespec *b)
{
    int64_t sec = (int64_t)a->tv_sec - (int64_t)b->tv_sec;
    int64_t nsec = (int64_t)a->tv_nsec - (int64_t)b->tv_nsec;
    return sec * NSEC_PER_SEC + nsec;
}

static void test_realtime_advance_recede(void)
{
    printf("Test 1: clock_settime realtime advance/recede\n");
    struct timespec before;
    struct timespec target;
    struct timespec after;
    int64_t drift;

    assert(clock_gettime(CLOCK_REALTIME, &before) == 0);

    target = before;
    target.tv_sec += 2;
    assert(clock_settime(CLOCK_REALTIME, &target) == 0);

    assert(clock_gettime(CLOCK_REALTIME, &after) == 0);
    drift = diff_ns(&after, &target);
    assert(drift >= 0);
    assert(drift <= ALLOWED_DRIFT_NS);

    target = before;
    if (target.tv_sec > 1) {
        target.tv_sec -= 1;
    } else {
        target.tv_sec = 0;
        target.tv_nsec = 0;
    }
    assert(clock_settime(CLOCK_REALTIME, &target) == 0);

    assert(clock_gettime(CLOCK_REALTIME, &after) == 0);
    drift = diff_ns(&after, &target);
    assert(drift >= 0);
    assert(drift <= ALLOWED_DRIFT_NS);

    assert(clock_settime(CLOCK_REALTIME, &before) == 0);
}

static void test_invalid_timespec(void)
{
    printf("Test 2: invalid timespec values\n");
    struct timespec ts;

    ts.tv_sec = -1;
    ts.tv_nsec = 0;
    errno = 0;
    assert(clock_settime(CLOCK_REALTIME, &ts) == -1);
    assert(errno == EINVAL);

    ts.tv_sec = 0;
    ts.tv_nsec = -1;
    errno = 0;
    assert(clock_settime(CLOCK_REALTIME, &ts) == -1);
    assert(errno == EINVAL);

    ts.tv_sec = 0;
    ts.tv_nsec = (long)NSEC_PER_SEC;
    errno = 0;
    assert(clock_settime(CLOCK_REALTIME, &ts) == -1);
    assert(errno == EINVAL);
}

static void test_invalid_clock_id(void)
{
    printf("Test 3: invalid clock id\n");
    struct timespec ts = {.tv_sec = 0, .tv_nsec = 0};

    errno = 0;
    assert(clock_settime((clockid_t)-1, &ts) == -1);
    assert(errno == EINVAL);
}

static void test_unsettable_clocks(void)
{
    printf("Test 4: unsettable clocks\n");
    struct timespec ts;

    assert(clock_gettime(CLOCK_REALTIME, &ts) == 0);

    errno = 0;
    assert(clock_settime(CLOCK_MONOTONIC, &ts) == -1);
    assert(errno == EINVAL);

    errno = 0;
    assert(clock_settime(CLOCK_PROCESS_CPUTIME_ID, &ts) == -1);
    assert(errno == EINVAL);

    errno = 0;
    assert(clock_settime(CLOCK_THREAD_CPUTIME_ID, &ts) == -1);
    assert(errno == EINVAL);
}

int main(void)
{
    test_realtime_advance_recede();
    test_invalid_timespec();
    test_invalid_clock_id();
    test_unsettable_clocks();
    printf("All tests passed!\n");
    return 0;
}
