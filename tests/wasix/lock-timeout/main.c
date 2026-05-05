#include <stdio.h>
#include <pthread.h>
#include <time.h>
#include <errno.h>

pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

void *f(void *arg) {
    (void)arg;

    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);

    ts.tv_nsec += 500 * 1000 * 1000; // 0.5 seconds
    if (ts.tv_nsec >= 1000000000) {
        ts.tv_sec += 1;
        ts.tv_nsec -= 1000000000;
    }

    int result = pthread_mutex_timedlock(&lock, &ts);

    return NULL;
}

int main(int argc, char** argv) {
    pthread_mutex_lock(&lock);

    pthread_t thread;
    pthread_create(&thread, NULL, f, NULL);

    pthread_join(thread, NULL);

    return 0;
}