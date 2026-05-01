#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test context switching with pthread mutexes

wasix_context_id_t ctx1, ctx2;
pthread_mutex_t shared_mutex;
int shared_counter = 0;

void context1_fn(void) {
  // Lock mutex in context 1
  int ret = pthread_mutex_lock(&shared_mutex);
  assert(ret == 0 && "Failed to lock mutex in context 1");

  shared_counter++;
  assert(shared_counter == 1 && "Counter should be 1");

  // Switch to context 2 while holding the mutex
  // Context 2 should be able to unlock it (same thread, shared mutex state)
  wasix_context_switch(ctx2);

  // After resuming, verify counter was updated by context 2
  assert(shared_counter == 2 && "Counter should be 2 after context 2");

  // Lock again to verify mutex is still usable
  ret = pthread_mutex_lock(&shared_mutex);
  assert(ret == 0 && "Failed to lock mutex again in context 1");

  pthread_mutex_unlock(&shared_mutex);

  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  // Unlock the mutex that was locked by context 1
  // This tests that mutex state is shared across contexts (same thread)
  int ret = pthread_mutex_unlock(&shared_mutex);
  assert(ret == 0 && "Failed to unlock mutex in context 2");

  shared_counter++;
  assert(shared_counter == 2 && "Counter should be 2");

  // Switch back to context 1 without the mutex locked
  wasix_context_switch(ctx1);
}

int main() {
  int ret;

  // Initialize mutex
  pthread_mutex_init(&shared_mutex, NULL);

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  // Start execution
  wasix_context_switch(ctx1);

  // Verify final state
  assert(shared_counter == 2 && "Final counter should be 2");

  // Cleanup
  pthread_mutex_destroy(&shared_mutex);
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Mutex switching test passed\n");
  return 0;
}
