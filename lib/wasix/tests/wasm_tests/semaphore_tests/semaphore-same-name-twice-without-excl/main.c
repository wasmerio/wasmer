#include <stdio.h>
#include <stdlib.h>
#include <pthread.h>
#include <semaphore.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <string.h>

int main(void) {
    puts("opening the first time");
    sem_t* sem1 = sem_open("/valid", O_CREAT | O_EXCL, 0600, 0);
    if (sem1 == SEM_FAILED) {
        perror("sem_open");
        sem_unlink("/valid"); // Don't check for errors, just best-effort cleanup
        return EXIT_FAILURE;
    }

    puts("opening a second time");
    // This one is expected to fail, because the name is already taken and O_EXCL was specified
    sem_t* sem2 = sem_open("/valid", O_CREAT, 0600, 0);
    if (sem2 == SEM_FAILED) {
        perror("sem_open");
        sem_unlink("/valid"); // Don't check for errors, just best-effort cleanup
        return EXIT_FAILURE;
    }

    sem_unlink("/valid"); // Don't check for errors, just best-effort cleanup
    puts("done.");
    return EXIT_SUCCESS;
}