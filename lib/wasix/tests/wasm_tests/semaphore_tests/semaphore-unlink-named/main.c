// Assert that the edgecases when unlinking a semaphore work the same as on native

#include <stdio.h>
#include <stdlib.h>
#include <pthread.h>
#include <semaphore.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <string.h>
#include <assert.h>

#define SEM_NAME "/test_unlink_unnamed_sem"
#define SEM_NAME_NON "/test_nonexistent_sem"

int main(void) {
    sem_t* sem = sem_open(SEM_NAME, O_CREAT | O_EXCL, 0600, 0);
    if (sem == SEM_FAILED) {
        perror("sem_open");
        return EXIT_FAILURE;
    }
    printf("Unlinking semaphore the first time\n");
    if (sem_unlink(SEM_NAME) == -1) {
        perror("sem_unlink");
        return EXIT_FAILURE;
    }

    printf("Unlinking semaphore again\n");
    assert(sem_unlink(SEM_NAME) == -1);
    assert(errno == ENOENT);

    puts("done.");
    return EXIT_SUCCESS;
}