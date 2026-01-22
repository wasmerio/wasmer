#include <assert.h>
#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <wasi/api_wasi.h>

static atomic_bool done;

static void *worker(void *arg) {
    (void)arg;
    __wasi_subscription_t sub = {0};
    __wasi_event_t out = {0};
    __wasi_size_t events = 0;
    const __wasi_size_t nsubscriptions = 0; // Passing a 0 causes an infinite loop
    __wasi_errno_t ret = __wasi_poll_oneoff(&sub, &out, nsubscriptions, &events);
    assert(ret == __WASI_ERRNO_INVAL && "poll_oneoff(0) should return EINVAL");
    atomic_store(&done, true);
    return NULL;
}

int main(void) {
    pthread_t thread;
    atomic_init(&done, false);
    assert(pthread_create(&thread, NULL, worker, NULL) == 0 && "pthread_create failed");

    __wasi_timestamp_t start = 0;
    assert(__wasi_clock_time_get(__WASI_CLOCKID_MONOTONIC, 1, &start)
               == __WASI_ERRNO_SUCCESS
           && "clock_time_get failed");
    while (!atomic_load(&done)) {
        __wasi_timestamp_t now = 0;
        assert(__wasi_clock_time_get(__WASI_CLOCKID_MONOTONIC, 1, &now)
                   == __WASI_ERRNO_SUCCESS
               && "clock_time_get failed");
        if (now - start > 1000000000ULL) {
            assert(0 && "poll_oneoff(0) did not return in time");
        }
        sched_yield();
    }

    assert(pthread_join(thread, NULL) == 0 && "pthread_join failed");
    return 0;
}
