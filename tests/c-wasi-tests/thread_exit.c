#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>

static volatile int after_exit_ran = 0;
static volatile int cleanup_ran = 0;
static volatile int tsd_ran = 0;
static pthread_key_t key;

static void cleanup(void *arg)
{
    (void)arg;
    cleanup_ran = 1;
}

static void dtor(void *p)
{
    int *v = (int *)p;
    *v = 1;
    tsd_ran = 1;
}

static void *thread_exit_value(void *arg)
{
    (void)arg;
    pthread_exit((void *)(uintptr_t)0x1234);
    after_exit_ran = 1;
    return (void *)0;
}

static void *thread_cleanup(void *arg)
{
    pthread_cleanup_push(cleanup, arg);
    pthread_exit(0);
    pthread_cleanup_pop(0);
    return arg;
}

static void *thread_tsd(void *arg)
{
    int *val = (int *)arg;
    assert(pthread_setspecific(key, val) == 0);
    pthread_exit(0);
    return arg;
}

static void test_exit_value(void)
{
    printf("Test 1: pthread_exit returns value to join\n");
    pthread_t t;
    void *ret = 0;
    assert(pthread_create(&t, 0, thread_exit_value, 0) == 0);
    assert(pthread_join(t, &ret) == 0);
    assert((uintptr_t)ret == 0x1234);
    assert(after_exit_ran == 0);
}

static void test_cleanup_handler(void)
{
    printf("Test 2: cleanup handler runs on pthread_exit\n");
    pthread_t t;
    cleanup_ran = 0;
    assert(pthread_create(&t, 0, thread_cleanup, 0) == 0);
    assert(pthread_join(t, 0) == 0);
    assert(cleanup_ran == 1);
}

static void test_tsd_destructor(void)
{
    printf("Test 3: TSD destructor runs on pthread_exit\n");
    pthread_t t;
    int val = 0;
    tsd_ran = 0;
    assert(pthread_key_create(&key, dtor) == 0);
    assert(pthread_create(&t, 0, thread_tsd, &val) == 0);
    assert(pthread_join(t, 0) == 0);
    assert(val == 1);
    assert(tsd_ran == 1);
}

int main(void)
{
    test_exit_value();
    test_cleanup_handler();
    test_tsd_destructor();
    printf("All tests passed!\n");
    return 0;
}
