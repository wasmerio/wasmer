#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test that context state (local variables, stack) is preserved across switches

wasix_context_id_t ctx1, ctx2;

void context1_fn(void) {
  int local1 = 100;
  int local2 = 200;
  int local3 = 300;

  // Switch to context2
  wasix_context_switch(ctx2);

  // After returning, verify our local variables are preserved
  assert(local1 == 100 && "Local variable 1 should be preserved");
  assert(local2 == 200 && "Local variable 2 should be preserved");
  assert(local3 == 300 && "Local variable 3 should be preserved");

  // Modify the variables
  local1 = 111;
  local2 = 222;
  local3 = 333;

  // Switch again
  wasix_context_switch(ctx2);

  // Verify modified values are preserved
  assert(local1 == 111 && "Modified local variable 1 should be preserved");
  assert(local2 == 222 && "Modified local variable 2 should be preserved");
  assert(local3 == 333 && "Modified local variable 3 should be preserved");

  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  int local_a = 10;
  int local_b = 20;

  // Switch back to context1
  wasix_context_switch(ctx1);

  // After returning, verify our local variables are preserved
  assert(local_a == 10 && "Local variable a should be preserved");
  assert(local_b == 20 && "Local variable b should be preserved");

  // Modify
  local_a = 99;
  local_b = 88;

  // Switch again
  wasix_context_switch(ctx1);

  // Verify modified values
  assert(local_a == 99 && "Modified local variable a should be preserved");
  assert(local_b == 88 && "Modified local variable b should be preserved");

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

  fprintf(stderr, "Context state preservation test passed\n");
  return 0;
}
