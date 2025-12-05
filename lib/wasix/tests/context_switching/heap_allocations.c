#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wasix/context.h>

// Test that contexts maintain independent heap allocations

wasix_context_id_t ctx1, ctx2;

#define BUFFER_SIZE 1024

void context1_fn(void) {
  // Allocate and fill memory
  char *buffer1 = (char *)malloc(BUFFER_SIZE);
  assert(buffer1 != NULL && "Failed to allocate buffer in context 1");

  memset(buffer1, 'A', BUFFER_SIZE);
  buffer1[BUFFER_SIZE - 1] = '\0';

  // Switch to context 2
  wasix_context_switch(ctx2);

  // Verify our buffer is still intact after context 2 executed
  assert(buffer1[0] == 'A' && "Buffer corrupted after context switch");
  assert(buffer1[BUFFER_SIZE - 2] == 'A' &&
         "Buffer corrupted after context switch");

  free(buffer1);
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  // Allocate and fill memory with different pattern
  char *buffer2 = (char *)malloc(BUFFER_SIZE * 2);
  assert(buffer2 != NULL && "Failed to allocate buffer in context 2");

  memset(buffer2, 'B', BUFFER_SIZE * 2);
  buffer2[BUFFER_SIZE * 2 - 1] = '\0';

  // Switch back to context 1
  wasix_context_switch(ctx1);

  // Verify our buffer is still intact
  assert(buffer2[0] == 'B' && "Buffer corrupted after context switch");
  assert(buffer2[BUFFER_SIZE * 2 - 2] == 'B' &&
         "Buffer corrupted after context switch");

  free(buffer2);
  wasix_context_switch(wasix_context_main);
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

  fprintf(stderr, "Heap allocations test passed\n");
  return 0;
}
