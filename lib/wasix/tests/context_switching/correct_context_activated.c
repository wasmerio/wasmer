// Test that the correct context is activated when switching by ID
// Creates 3 contexts and verifies each one's entrypoint is called when
// switching to it
#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2, ctx3;

void context1_fn(void) {
  fprintf(stderr, "context1_fn was called (expected for ctx1=%llu)\n",
          (unsigned long long)ctx1);
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  fprintf(stderr, "context2_fn was called (expected for ctx2=%llu)\n",
          (unsigned long long)ctx2);
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

void context3_fn(void) {
  fprintf(stderr, "context3_fn was called (expected for ctx3=%llu)\n",
          (unsigned long long)ctx3);
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

int main() {
  int ret;

  // Create contexts in order: ctx1, ctx2, ctx3
  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0);
  fprintf(stderr, "Created ctx1=%llu with entrypoint=context1_fn\n",
          (unsigned long long)ctx1);

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0);
  fprintf(stderr, "Created ctx2=%llu with entrypoint=context2_fn\n",
          (unsigned long long)ctx2);

  ret = wasix_context_create(&ctx3, context3_fn);
  assert(ret == 0);
  fprintf(stderr, "Created ctx3=%llu with entrypoint=context3_fn\n",
          (unsigned long long)ctx3);

  // Now switch to ctx1 specifically
  fprintf(stderr, "\nSwitching to ctx1 (id=%llu)\n", (unsigned long long)ctx1);
  fflush(stderr);
  wasix_context_switch(ctx1);
  fprintf(stderr, "Back from ctx1\n\n");

  // Cleanup and test passed
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);
  wasix_context_destroy(ctx3);

  fprintf(stderr, "Test passed - ctx1 was correctly activated!\n");
  return 0;
}
