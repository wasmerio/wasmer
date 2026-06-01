// Regression test for issue where wasix-libc would set 0x3fffffff as the TID
// for the main thread, leading to a clash with pthread's mutex poison tracking
// code

#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <string.h>

int main(void) {
  pthread_mutexattr_t attr;
  pthread_mutex_t mutex;

  int r = pthread_mutexattr_init(&attr);
  if (r) {
    printf("pthread_mutexattr_init: %s\n", strerror(r));
    return 1;
  }

  r = pthread_mutexattr_settype(&attr, PTHREAD_MUTEX_ERRORCHECK);
  if (r) {
    printf("pthread_mutexattr_settype: %s\n", strerror(r));
    return 1;
  }

  r = pthread_mutex_init(&mutex, &attr);
  if (r) {
    printf("pthread_mutex_init: %s\n", strerror(r));
    return 1;
  }

  r = pthread_mutex_lock(&mutex);
  if (r) {
    printf("pthread_mutex_lock: %s\n", strerror(r));
    return 1;
  }

  r = pthread_mutex_trylock(&mutex);

  if (r == ENOTRECOVERABLE) {
    printf("BUG: pthread_mutex_trylock returned ENOTRECOVERABLE\n");
    return 1;
  }

  if (r != EBUSY) {
    printf("BUG: pthread_mutex_trylock returned %d (%s), expected EBUSY\n", r,
           strerror(r));
    return 1;
  }

  printf("ok\n");
  return 0;
}