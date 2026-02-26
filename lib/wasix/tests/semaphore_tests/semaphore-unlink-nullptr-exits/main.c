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

#define SEM_NAME_NON "/test_nonexistent_sem"

int main(void) {
    printf("Unlinking nullptr\n");
    // Unlinking a nullpointer causes a segmentation fault on native, so it should also cause some sort of exit on WASIX
    assert(sem_unlink(NULL) == -1);
    assert(errno == ENOENT);

    puts("Should not reach this");
    return EXIT_SUCCESS;
}
