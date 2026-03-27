#include <stdio.h>
#include <stdlib.h>
#include <pthread.h>
#include <semaphore.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <string.h>

int main(void) {
    sem_t* sem = sem_open("/without-create", O_EXCL, 0600, 0);
    if (sem != SEM_FAILED) {
        fprintf(stderr, "sem_open worked even without _CREAT\n");
        sem_unlink("/without-create"); // Don't check for errors, just best-effort cleanup
        return EXIT_FAILURE;
    }

    sem_unlink("/without-create"); // Don't check for errors, just best-effort cleanup
    puts("done.");
    return EXIT_SUCCESS;
}