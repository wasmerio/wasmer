#include <stdio.h>
#include <stdlib.h>
#include <pthread.h>
#include <semaphore.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <string.h>

int main(void) {
    sem_t* sem1 = sem_open("/valid", O_CREAT | O_EXCL, 0600, 0);
    if (sem1 == SEM_FAILED) {
        perror("sem_open");
        sem_unlink("/valid"); // Don't check for errors, just best-effort cleanup
        return EXIT_FAILURE;
    }

    // This one is expected to fail, because the name is already taken and O_EXCL was specified
    sem_t* sem2 = sem_open("/valid", O_CREAT | O_EXCL, 0600, 0);
    if (sem2 != SEM_FAILED) {
        fprintf(stderr, "sem_open twice with same name and O_EXCL did not fail\n");
        sem_unlink("/valid"); // Don't check for errors, just best-effort cleanup
        return EXIT_FAILURE;
    }

    sem_unlink("/valid"); // Don't check for errors, just best-effort cleanup
    puts("done.");
    return EXIT_SUCCESS;
}