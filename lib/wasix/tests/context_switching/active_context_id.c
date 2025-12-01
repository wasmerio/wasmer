// Test that wasix_context_main always returns the main context ID
// regardless of which context is currently active
#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2;
wasix_context_id_t main_ctx_id;
int phase = 0;

void context1_fn(void) {
  phase = 1;

  // wasix_context_main should always return the main context's ID,
  // even when running in ctx1
  wasix_context_id_t main_id = wasix_context_main;
  fprintf(stderr,
          "Phase 1: wasix_context_main in ctx1 = %llu (expected %llu)\n",
          (unsigned long long)main_id, (unsigned long long)main_ctx_id);

  // This assertion verifies that wasix_context_main returns the main context
  assert(main_id == main_ctx_id &&
         "wasix_context_main should return main context ID even in ctx1");

  wasix_context_switch(ctx2);

  phase = 3;
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  phase = 2;

  // wasix_context_main should always return the main context's ID,
  // even when running in ctx2
  wasix_context_id_t main_id = wasix_context_main;
  fprintf(stderr,
          "Phase 2: wasix_context_main in ctx2 = %llu (expected %llu)\n",
          (unsigned long long)main_id, (unsigned long long)main_ctx_id);

  // This assertion verifies that wasix_context_main returns the main context
  assert(main_id == main_ctx_id &&
         "wasix_context_main should return main context ID even in ctx2");

  wasix_context_switch(ctx1);
}

int main() {
  int ret;

  // Store the main context ID for comparison in other contexts
  main_ctx_id = wasix_context_main;
  fprintf(stderr, "Main context ID = %llu\n", (unsigned long long)main_ctx_id);

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  // Start execution
  wasix_context_switch(ctx1);

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "wasix_context_main test passed\n");
  return 0;
}
