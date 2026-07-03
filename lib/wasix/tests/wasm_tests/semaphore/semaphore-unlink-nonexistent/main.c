// Assert that the edgecases when unlinking a semaphore work the same as on
// native

#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <semaphore.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define SEM_NAME_NON "/test_nonexistent_sem"

int main(void) {
  printf("Unlinking nonexistent semaphore\n");
  assert(sem_unlink(SEM_NAME_NON) == -1);
  assert(errno == ENOENT);

  puts("done.");
  return EXIT_SUCCESS;
}