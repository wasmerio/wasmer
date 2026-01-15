#include <errno.h>
#include <pthread.h>
#include <semaphore.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define TOKENS 5

static sem_t sem;

static int sem_wait_checked(sem_t* s) {
    for (;;) {
        if (sem_wait(s) == 0) return 0;
        if (errno == EINTR) continue;   // retry if interrupted by signal
        perror("sem_wait");
        return -1;
    }
}

void* worker(void* arg) {
    (void)arg;
    for (int i = 1; i <= TOKENS; i++) {
        if (sem_wait_checked(&sem) == -1) {
            // Return a non-null value to signal error to the joiner
            pthread_exit((void*)1);
        }
        printf("worker: got token %d\n", i);
        usleep(100 * 1000);
    }
    return NULL;
}

int main(void) {
    // Initialize unnamed semaphore for intra-process use (pshared = 0), initial value = 0
    if (sem_init(&sem, 0, 0) == -1) {
        perror("sem_init");
        return EXIT_FAILURE;
    }

    pthread_t th;
    int rc = pthread_create(&th, NULL, worker, NULL);
    if (rc != 0) {
        fprintf(stderr, "pthread_create: %s\n", strerror(rc));
        if (sem_destroy(&sem) == -1) perror("sem_destroy");
        return EXIT_FAILURE;
    }

    // Let worker start and block on sem_wait
    usleep(100 * 1000);

    for (int i = 1; i <= TOKENS; i++) {
        printf("main: posting token %d\n", i);
        if (sem_post(&sem) == -1) {
            perror("sem_post");
            // Best-effort cleanup: stop worker, join, then destroy semaphore
            pthread_cancel(th);
            pthread_join(th, NULL);
            if (sem_destroy(&sem) == -1) perror("sem_destroy");
            return EXIT_FAILURE;
        }
        usleep(50 * 1000);
    }

    void* thread_ret = NULL;
    rc = pthread_join(th, &thread_ret);
    if (rc != 0) {
        fprintf(stderr, "pthread_join: %s\n", strerror(rc));
        if (sem_destroy(&sem) == -1) perror("sem_destroy");
        return EXIT_FAILURE;
    }
    if (thread_ret != NULL) {
        fprintf(stderr, "worker thread reported an error\n");
        if (sem_destroy(&sem) == -1) perror("sem_destroy");
        return EXIT_FAILURE;
    }
    if (sem_destroy(&sem) == -1) {
        perror("sem_destroy");
        return EXIT_FAILURE;
    }

    puts("done.");
    return EXIT_SUCCESS;
}