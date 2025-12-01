// Test that active_context_id is updated correctly during context switches
// This is a simple test to verify the implementation bug where
// current_context_id is not updated when switching to a target context
#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2;
int phase = 0;

void context1_fn(void) {
  phase = 1;

  // When ctx1 is running, wasix_context_main should return ctx1's ID
  wasix_context_id_t active_id = wasix_context_main;
  fprintf(stderr, "Phase 1: active context ID in ctx1 = %llu (expected %llu)\n",
          (unsigned long long)active_id, (unsigned long long)ctx1);

  // This assertion should pass - the active context should be ctx1
  assert(active_id == ctx1 &&
         "Active context should be ctx1 when running in ctx1");

  wasix_context_switch(ctx2);

  phase = 3;
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  phase = 2;

  // When ctx2 is running, wasix_context_main should return ctx2's ID
  wasix_context_id_t active_id = wasix_context_main;
  fprintf(stderr, "Phase 2: active context ID in ctx2 = %llu (expected %llu)\n",
          (unsigned long long)active_id, (unsigned long long)ctx2);

  // This assertion exposes the bug - active context is still ctx1 instead of
  // ctx2
  assert(active_id == ctx2 &&
         "Active context should be ctx2 when running in ctx2");

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

  fprintf(stderr, "Active context ID test passed\n");
  return 0;
}
