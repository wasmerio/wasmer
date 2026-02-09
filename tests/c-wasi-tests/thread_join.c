#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <unistd.h>
#include <errno.h>

static volatile int thread_started = 0;
static volatile int thread_done = 0;

static void *thread_wait(void *arg)
{
    (void)arg;
    thread_started = 1;
    usleep(200000);
    thread_done = 1;
    return 0;
}

static void *thread_return_value(void *arg)
{
    (void)arg;
    return (void *)(uintptr_t)0x1234;
}

static void *thread_quick_exit(void *arg)
{
    (void)arg;
    return 0;
}

static void test_join_waits(void)
{
    printf("Test 1: pthread_join waits for thread to finish\n");
    pthread_t t;
    thread_started = 0;
    thread_done = 0;
    assert(pthread_create(&t, 0, thread_wait, 0) == 0);
    while (!thread_started) {
        usleep(1000);
    }
    assert(pthread_join(t, 0) == 0);
    assert(thread_done == 1);
}

static void test_join_return_value(void)
{
    printf("Test 2: pthread_join returns thread value\n");
    pthread_t t;
    void *ret = 0;
    assert(pthread_create(&t, 0, thread_return_value, 0) == 0);
    assert(pthread_join(t, &ret) == 0);
    assert((uintptr_t)ret == 0x1234);
}

static void test_join_detached(void)
{
    printf("Test 3: pthread_join on detached thread returns EINVAL\n");
    pthread_t t;
    pthread_attr_t attr;
    assert(pthread_attr_init(&attr) == 0);
    assert(pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED) == 0);
    assert(pthread_create(&t, &attr, thread_quick_exit, 0) == 0);
    int ret = pthread_join(t, 0);
    assert(ret == EINVAL);
    assert(pthread_attr_destroy(&attr) == 0);
}

static void test_join_twice(void)
{
    printf("Test 4: pthread_join twice returns ESRCH\n");
    pthread_t t;
    assert(pthread_create(&t, 0, thread_quick_exit, 0) == 0);
    assert(pthread_join(t, 0) == 0);
    int ret = pthread_join(t, 0);
    assert(ret == ESRCH);
}

int main(void)
{
    test_join_waits();
    test_join_return_value();
    test_join_detached();
    test_join_twice();
    printf("All tests passed!\n");
    return 0;
}
