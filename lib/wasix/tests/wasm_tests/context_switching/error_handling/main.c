#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test error handling: invalid operations should fail with appropriate errors

wasix_context_id_t ctx1;
int test_phase = 0;

void context1_fn(void) {
  int ret;

  // Test: Cannot destroy the active context (self)
  ret = wasix_context_destroy(ctx1);
  assert(ret == -1 && "Should fail to destroy active context");
  assert(errno == EINVAL &&
         "Should set errno to EINVAL for active context destroy");

  test_phase = 1;
  wasix_context_switch(wasix_context_main);
}

int main() {
  int ret;

  // Create a context
  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context");

  // Switch to context to test destroying active context
  wasix_context_switch(ctx1);
  assert(test_phase == 1 && "Context did not execute");

  // Test: Cannot destroy main context
  ret = wasix_context_destroy(wasix_context_main);
  assert(ret == -1 && "Should fail to destroy main context");
  assert(errno == EINVAL &&
         "Should set errno to EINVAL for main context destroy");

  // Destroy the context
  ret = wasix_context_destroy(ctx1);
  assert(ret == 0 && "Failed to destroy context");

  // Test: Switching to a destroyed context should fail
  errno = 0;
  ret = wasix_context_switch(ctx1);
  assert(ret == -1 && "Should fail to switch to destroyed context");
  assert(errno == EINVAL && "Should set errno to EINVAL for destroyed context");

  // Test: Destroying an already destroyed context is a no-op
  ret = wasix_context_destroy(ctx1);
  assert(ret == 0 &&
         "Destroying already destroyed context should succeed (no-op)");

  fprintf(stderr, "All error handling tests passed\n");
  return 0;
}
