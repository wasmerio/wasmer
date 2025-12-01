#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test switching to the currently active context (should be a no-op)

wasix_context_id_t ctx1;
int switch_count = 0;

void context1_fn(void) {
  int local_var = 42;

  // Switch to self (should be a no-op)
  int ret = wasix_context_switch(ctx1);
  assert(ret == 0 && "Self-switch should succeed");

  // Verify we're still in the same context
  assert(local_var == 42 &&
         "Local variable should be unchanged after self-switch");
  switch_count++;

  // Try again
  ret = wasix_context_switch(ctx1);
  assert(ret == 0 && "Second self-switch should succeed");
  switch_count++;

  wasix_context_switch(wasix_context_main);
}

int main() {
  int ret;
  int main_local = 123;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  // Test self-switch in main context
  ret = wasix_context_switch(wasix_context_main);
  assert(ret == 0 && "Main context self-switch should succeed");
  assert(main_local == 123 && "Main local variable should be unchanged");

  // Switch to context 1
  wasix_context_switch(ctx1);

  // Verify context1 executed
  assert(switch_count == 2 &&
         "Context 1 should have performed 2 self-switches");

  // Cleanup
  wasix_context_destroy(ctx1);

  fprintf(stderr, "Self-switching test passed\n");
  return 0;
}
