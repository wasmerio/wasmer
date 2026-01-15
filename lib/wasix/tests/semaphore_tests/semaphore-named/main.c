// named_sem_test.c
// Build:  gcc -pthread named_sem_test.c -o named_sem_test
// Run:    ./named_sem_test
//
// Demonstrates a named semaphore between threads.
// Includes exit code checks for all semaphore-related calls.

#include <stdio.h>
#include <stdlib.h>
#include <pthread.h>
#include <semaphore.h>
#include <fcntl.h>      // For O_CREAT, O_EXCL
#include <unistd.h>
#include <errno.h>
#include <string.h>

#define SEM_NAME "/test_named_sems"
#define TOKENS 5

void* worker(void* arg) {
    sem_t* sem = (sem_t*)arg;
    for (int i = 1; i <= TOKENS; i++) {
        if (sem_wait(sem) < 0) {
            perror("sem_wait");
            pthread_exit((void*)1);
        }
        printf("worker: got token %d\n", i);
        usleep(100 * 1000);
    }
    return NULL;
}

int main(void) {
    sem_t* sem = sem_open(SEM_NAME, O_CREAT | O_EXCL, 0600, 0);
    if (sem == SEM_FAILED) {
        perror("sem_open");
        return EXIT_FAILURE;
    }

    pthread_t th;
    if (pthread_create(&th, NULL, worker, sem) != 0) {
        perror("pthread_create");
        if (sem_close(sem) < 0) perror("sem_close");
        if (sem_unlink(SEM_NAME) < 0) perror("sem_unlink");
        return EXIT_FAILURE;
    }

    usleep(100 * 1000); // let worker block

    for (int i = 1; i <= TOKENS; i++) {
        printf("main: posting token %d\n", i);
        if (sem_post(sem) < 0) {
            perror("sem_post");
            pthread_cancel(th);
            pthread_join(th, NULL);
            if (sem_close(sem) < 0) perror("sem_close");
            if (sem_unlink(SEM_NAME) < 0) perror("sem_unlink");
            return EXIT_FAILURE;
        }
        usleep(50 * 1000);
    }

    void* thread_ret;
    if (pthread_join(th, &thread_ret) != 0) {
        perror("pthread_join");
        if (sem_close(sem) < 0) perror("sem_close");
        if (sem_unlink(SEM_NAME) < 0) perror("sem_unlink");
        return EXIT_FAILURE;
    }
    if (thread_ret != NULL) {
        fprintf(stderr, "Worker thread exited with error\n");
        if (sem_close(sem) < 0) perror("sem_close");
        if (sem_unlink(SEM_NAME) < 0) perror("sem_unlink");
        return EXIT_FAILURE;
    }

    if (sem_close(sem) < 0) {
        perror("sem_close");
        return EXIT_FAILURE;
    }
    if (sem_unlink(SEM_NAME) < 0) {
        perror("sem_unlink");
        return EXIT_FAILURE;
    }

    puts("done.");
    return EXIT_SUCCESS;
}