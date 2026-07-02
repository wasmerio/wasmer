#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <semaphore.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(void) {
  puts("opening the first time with O_CREAT");
  sem_t* sem1 = sem_open("/open-with-create", O_CREAT, 0600, 0);
  if (sem1 == SEM_FAILED) {
    perror("sem_open");
    sem_unlink("/open-with-create");  // Don't check for errors, just
                                      // best-effort cleanup
    return EXIT_FAILURE;
  }

  puts("opening a second time without O_CREAT");
  // This one is expected to fail, because the name is already taken and O_EXCL
  // was specified
  sem_t* sem2 = sem_open("/open-with-create", 0, 0600, 0);
  if (sem2 == SEM_FAILED) {
    perror("sem_open");
    sem_unlink("/open-with-create");  // Don't check for errors, just
                                      // best-effort cleanup
    return EXIT_FAILURE;
  }

  sem_unlink(
      "/open-with-create");  // Don't check for errors, just best-effort cleanup
  puts("done.");
  return EXIT_SUCCESS;
}