// Spawns a thread whose stack, and with it the start arguments that libc
// passes to `thread_spawn`, lives above 2 GiB. wasm32 addresses in the upper
// half of the address space are valid and must reach `wasi_thread_start`
// unchanged.
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define TWO_GIB 0x80000000u
#define STACK_SIZE (1u << 20)
#define BLOCK_SIZE (256u << 20)
#define MAX_BLOCKS 16
#define WAIT_TIMEOUT_MS 30000

static atomic_int thread_ran = 0;

static void* thread_main(void* arg) {
  atomic_store(&thread_ran, 1);
  return arg;
}

int main(void) {
  // Grow the heap until malloc hands out memory above 2 GiB and use that
  // block as the thread stack. A single allocation cannot do this because
  // wasm32 sbrk() takes a signed increment. The blocks are never written, so
  // only the pages backing the stack get committed.
  unsigned char* blocks[MAX_BLOCKS];
  int block_count = 0;
  unsigned char* stack = NULL;
  while (stack == NULL) {
    if (block_count == MAX_BLOCKS) {
      fprintf(stderr, "heap did not reach 2 GiB\n");
      return 1;
    }
    unsigned char* block = malloc(BLOCK_SIZE);
    if (block == NULL) {
      perror("malloc");
      return 1;
    }
    blocks[block_count++] = block;
    if ((uintptr_t)block >= TWO_GIB) {
      stack = block;
    }
  }

  pthread_attr_t attr;
  int rc = pthread_attr_init(&attr);
  if (rc == 0) {
    rc = pthread_attr_setstack(&attr, stack, STACK_SIZE);
  }
  if (rc != 0) {
    fprintf(stderr, "pthread_attr: %s\n", strerror(rc));
    return 1;
  }

  void* const expected = (void*)(uintptr_t)0x1234;
  pthread_t thread;
  rc = pthread_create(&thread, &attr, thread_main, expected);
  if (rc != 0) {
    fprintf(stderr, "pthread_create: %s\n", strerror(rc));
    return 1;
  }

  // Fail instead of hanging in pthread_join if the thread never starts.
  const struct timespec tick = {.tv_sec = 0, .tv_nsec = 10 * 1000 * 1000};
  for (int waited_ms = 0; !atomic_load(&thread_ran); waited_ms += 10) {
    if (waited_ms >= WAIT_TIMEOUT_MS) {
      fprintf(stderr, "thread did not start\n");
      return 1;
    }
    nanosleep(&tick, NULL);
  }

  void* result;
  rc = pthread_join(thread, &result);
  if (rc != 0) {
    fprintf(stderr, "pthread_join: %s\n", strerror(rc));
    return 1;
  }
  if (result != expected) {
    fprintf(stderr, "thread returned %p, expected %p\n", result, expected);
    return 1;
  }

  pthread_attr_destroy(&attr);
  for (int i = 0; i < block_count; i++) {
    free(blocks[i]);
  }
  return 0;
}
