#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <stdio.h>

static volatile sig_atomic_t got_sig = 0;
static volatile int worker_ready = 0;
static volatile int worker_tls_snapshot = 0;
static volatile int signal_timeout = 0;
static __thread volatile sig_atomic_t tls_handled = 0;
static int failures = 0;

static void handler(int sig)
{
    got_sig = sig;
    tls_handled = 1;
}

static void *worker(void *arg)
{
    (void)arg;
    worker_ready = 1;
    while (!got_sig && !signal_timeout) {
        sched_yield();
    }
    worker_tls_snapshot = tls_handled;
    return 0;
}

static void *exiting_thread(void *arg)
{
    (void)arg;
    return 0;
}

static void test_signal_delivery(void)
{
    // From openposixtestsuite pthread_kill 1-1/1-2: signal delivered to target thread.
    printf("Test 1: signal delivered to target thread\n");
    struct sigaction sa;
    sa.sa_flags = 0;
    sa.sa_handler = handler;
    sigemptyset(&sa.sa_mask);
    assert(sigaction(SIGUSR1, &sa, 0) == 0);

    pthread_t t;
    got_sig = 0;
    worker_ready = 0;
    worker_tls_snapshot = 0;
    signal_timeout = 0;

    assert(pthread_create(&t, 0, worker, 0) == 0);
    while (!worker_ready) {
        sched_yield();
    }

    int r = pthread_kill(t, SIGUSR1);
    assert(r == 0);
    for (int i = 0; i < 1000000 && !got_sig; i++) {
        sched_yield();
    }
    if (!got_sig) {
        signal_timeout = 1;
    }
    assert(pthread_join(t, 0) == 0);
    if (got_sig != SIGUSR1) {
        fprintf(stderr, "signal not delivered (got_sig=%d)\n", (int)got_sig);
        failures++;
    }
    if (worker_tls_snapshot != 1) {
        fprintf(stderr, "signal not handled on target thread\n");
        failures++;
    }
}

static void test_null_signal(void)
{
    // From openposixtestsuite pthread_kill 2-1/3-1: sig=0 returns success.
    printf("Test 2: null signal returns success\n");
    int r = pthread_kill(pthread_self(), 0);
    if (r != 0) {
        fprintf(stderr, "pthread_kill(sig=0) expected 0, got %d\n", r);
        failures++;
    }
}

static void test_invalid_signal(void)
{
    // From openposixtestsuite pthread_kill 7-1: invalid signal -> EINVAL.
    printf("Test 3: invalid signal returns EINVAL\n");
    int r = pthread_kill(pthread_self(), -1);
    if (r != EINVAL) {
        fprintf(stderr, "pthread_kill(invalid sig) expected EINVAL, got %d\n", r);
        failures++;
    }
}

static void test_esrch_after_exit(void)
{
    // From openposixtestsuite pthread_kill 6-1: ESRCH for exited thread.
    printf("Test 4: ESRCH after thread exit\n");
    pthread_t t;
    assert(pthread_create(&t, 0, exiting_thread, 0) == 0);
    assert(pthread_join(t, 0) == 0);
    int r = pthread_kill(t, 0);
    if (r != ESRCH) {
        fprintf(stderr, "pthread_kill(exited thread) expected ESRCH, got %d\n", r);
        failures++;
    }
}

int main(void)
{
    test_signal_delivery();
    test_null_signal();
    test_invalid_signal();
    test_esrch_after_exit();
    if (failures != 0) {
        fprintf(stderr, "%d thread_signal check(s) failed\n", failures);
        assert(0);
    }
    printf("All tests passed!\n");
    return 0;
}
