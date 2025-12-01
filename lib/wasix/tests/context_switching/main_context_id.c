#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test wasix_context_main identifier behavior

wasix_context_id_t ctx1;
wasix_context_id_t main_ctx_from_ctx1;
wasix_context_id_t main_ctx_from_main;

void context1_fn(void) {
  // Get main context identifier from within a non-main context
  main_ctx_from_ctx1 = wasix_context_main;

  // Switch back to main
  wasix_context_switch(wasix_context_main);

  // This should not be reached
  fprintf(stderr,
          "ERROR: Execution continued in context1 after switch to main\n");
  exit(1);
}

int main() {
  int ret;

  // Get main context identifier from main
  main_ctx_from_main = wasix_context_main;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  // Switch to context 1
  wasix_context_switch(ctx1);

  // Verify that wasix_context_main is consistent
  assert(main_ctx_from_main == main_ctx_from_ctx1 &&
         "wasix_context_main should be the same from all contexts");

  // Test that we can switch to main context using the identifier
  ret = wasix_context_create(
      &ctx1, context1_fn); // Recreate since previous one terminated
  assert(ret == 0 && "Failed to recreate context 1");

  wasix_context_switch(ctx1);

  // We should be back here after context1 switches to main

  // Cleanup
  wasix_context_destroy(ctx1);

  fprintf(stderr, "Main context identifier test passed\n");
  return 0;
}
