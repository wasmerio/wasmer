#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wasix/context.h>

// Test context switching with complex nested operations that might leave
// internal state borrowed

#define NUM_CONTEXTS 5

wasix_context_id_t contexts[NUM_CONTEXTS];
int execution_count = 0;

// Recursive function that does complex operations and switches contexts
void recursive_switch(int depth, int max_depth, wasix_context_id_t next_ctx) {
  char buffer[1024];

  if (depth >= max_depth) {
    execution_count++;
    // Switch to next context at maximum depth
    wasix_context_switch(next_ctx);
    return;
  }

  // Do some complex string operations
  snprintf(buffer, sizeof(buffer), "Depth %d recursion", depth);

  // Allocate and free memory
  void *ptr = malloc(1024);
  memset(ptr, depth, 1024);
  free(ptr);

  // Recurse
  recursive_switch(depth + 1, max_depth, next_ctx);

  // After recursion, do more operations
  snprintf(buffer, sizeof(buffer), "Returning from depth %d", depth);
}

void context0_fn(void) { recursive_switch(0, 10, contexts[1]); }

void context1_fn(void) { recursive_switch(0, 10, contexts[2]); }

void context2_fn(void) { recursive_switch(0, 10, contexts[3]); }

void context3_fn(void) { recursive_switch(0, 10, contexts[4]); }

void context4_fn(void) { recursive_switch(0, 10, wasix_context_main); }

int main() {
  int ret;

  // Create contexts with entrypoints
  void (*entrypoints[])(void) = {context0_fn, context1_fn, context2_fn,
                                 context3_fn, context4_fn};

  for (int i = 0; i < NUM_CONTEXTS; i++) {
    ret = wasix_context_create(&contexts[i], entrypoints[i]);
    assert(ret == 0 && "Failed to create context");
  }

  // Start the chain
  wasix_context_switch(contexts[0]);

  // Verify all contexts executed
  assert(execution_count == NUM_CONTEXTS &&
         "All contexts should have executed");

  // Cleanup
  for (int i = 0; i < NUM_CONTEXTS; i++) {
    wasix_context_destroy(contexts[i]);
  }

  fprintf(stderr, "Complex nested operations test passed\n");
  return 0;
}
