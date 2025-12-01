#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test creating many contexts to stress resource management

#define NUM_CONTEXTS 20

wasix_context_id_t contexts[NUM_CONTEXTS];
int executed[NUM_CONTEXTS] = {0};

void generic_context_fn(void) {
  // Find which context this is
  for (int i = 0; i < NUM_CONTEXTS; i++) {
    if (!executed[i]) {
      executed[i] = 1;
      break;
    }
  }
  wasix_context_switch(wasix_context_main);
}

int main() {
  int ret;

  // Create many contexts
  for (int i = 0; i < NUM_CONTEXTS; i++) {
    ret = wasix_context_create(&contexts[i], generic_context_fn);
    assert(ret == 0 && "Failed to create context");
  }

  // Execute them all
  for (int i = 0; i < NUM_CONTEXTS; i++) {
    wasix_context_switch(contexts[i]);
    assert(executed[i] == 1 && "Context should have executed");
  }

  // Destroy them all
  for (int i = 0; i < NUM_CONTEXTS; i++) {
    ret = wasix_context_destroy(contexts[i]);
    assert(ret == 0 && "Failed to destroy context");
  }

  // Create another batch to ensure resources were properly freed
  for (int i = 0; i < NUM_CONTEXTS; i++) {
    ret = wasix_context_create(&contexts[i], generic_context_fn);
    assert(ret == 0 && "Failed to create context in second batch");
  }

  // Destroy second batch
  for (int i = 0; i < NUM_CONTEXTS; i++) {
    ret = wasix_context_destroy(contexts[i]);
    assert(ret == 0 && "Failed to destroy context in second batch");
  }

  fprintf(
      stderr,
      "Many contexts test passed (%d contexts created and destroyed twice)\n",
      NUM_CONTEXTS);
  return 0;
}
