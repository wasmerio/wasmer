#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>
#include <wasix/context.h>

// Test context switching with time operations

wasix_context_id_t ctx1, ctx2;

void context1_fn(void) {
  struct timespec start, end;

  // Get time before sleep
  clock_gettime(CLOCK_MONOTONIC, &start);

  // Sleep for a short time
  usleep(10000); // 10ms

  // Get time after sleep
  clock_gettime(CLOCK_MONOTONIC, &end);

  // Calculate elapsed time
  long elapsed_ns =
      (end.tv_sec - start.tv_sec) * 1000000000L + (end.tv_nsec - start.tv_nsec);
  assert(elapsed_ns >= 10000000 && "Sleep should take at least 10ms");

  // Switch to context 2
  wasix_context_switch(ctx2);

  // After resuming, do another timing operation
  clock_gettime(CLOCK_MONOTONIC, &start);
  usleep(5000); // 5ms
  clock_gettime(CLOCK_MONOTONIC, &end);

  elapsed_ns =
      (end.tv_sec - start.tv_sec) * 1000000000L + (end.tv_nsec - start.tv_nsec);
  assert(elapsed_ns >= 5000000 && "Second sleep should take at least 5ms");

  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  struct timespec ts;

  // Get current time
  clock_gettime(CLOCK_REALTIME, &ts);
  assert(ts.tv_sec > 0 && "Should have valid timestamp");

  // Do a small sleep
  usleep(3000); // 3ms

  // Switch back to context 1
  wasix_context_switch(ctx1);
}

int main() {
  int ret;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  // Start execution
  wasix_context_switch(ctx1);

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Timer operations switching test passed\n");
  return 0;
}
