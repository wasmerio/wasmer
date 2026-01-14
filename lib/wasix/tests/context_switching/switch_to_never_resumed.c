// Test that switching to a never-resumed context activates the correct context
// This test creates 3 contexts and switches between them to verify
// that the correct entrypoint is called for each
#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2, ctx3;
int execution_order[10];
int order_idx = 0;

void context1_fn(void) {
  fprintf(stderr, "context1_fn executing (ctx1=%llu)\n",
          (unsigned long long)ctx1);
  fflush(stderr);
  execution_order[order_idx++] = 1;

  // Switch to ctx2 (which has never been resumed before)
  fprintf(stderr, "ctx1 switching to ctx2 (id=%llu)\n",
          (unsigned long long)ctx2);
  fflush(stderr);
  wasix_context_switch(ctx2);

  // After ctx2 switches back to us
  fprintf(stderr, "ctx1 resumed\n");
  fflush(stderr);
  execution_order[order_idx++] = 4;
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  fprintf(stderr, "context2_fn executing (ctx2=%llu)\n",
          (unsigned long long)ctx2);
  fflush(stderr);
  execution_order[order_idx++] = 2;

  // Switch to ctx3 (which has never been resumed before)
  fprintf(stderr, "ctx2 switching to ctx3 (id=%llu)\n",
          (unsigned long long)ctx3);
  fflush(stderr);
  wasix_context_switch(ctx3);

  // Should not reach here in this test
  fprintf(stderr, "ERROR: ctx2 resumed unexpectedly\n");
  fflush(stderr);
  execution_order[order_idx++] = 99;
  wasix_context_switch(wasix_context_main);
}

void context3_fn(void) {
  fprintf(stderr, "context3_fn executing (ctx3=%llu)\n",
          (unsigned long long)ctx3);
  fflush(stderr);
  execution_order[order_idx++] = 3;

  // Switch back to ctx1
  fprintf(stderr, "ctx3 switching to ctx1 (id=%llu)\n",
          (unsigned long long)ctx1);
  fflush(stderr);
  wasix_context_switch(ctx1);

  // Should not reach here in this test
  fprintf(stderr, "ERROR: ctx3 resumed unexpectedly\n");
  fflush(stderr);
  execution_order[order_idx++] = 99;
  wasix_context_switch(wasix_context_main);
}

int main() {
  int ret;

  // Create all three contexts while in main
  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");
  fprintf(stderr, "Created ctx1 with id=%llu\n", (unsigned long long)ctx1);
  fflush(stderr);

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");
  fprintf(stderr, "Created ctx2 with id=%llu\n", (unsigned long long)ctx2);
  fflush(stderr);

  ret = wasix_context_create(&ctx3, context3_fn);
  assert(ret == 0 && "Failed to create context 3");
  fprintf(stderr, "Created ctx3 with id=%llu\n", (unsigned long long)ctx3);
  fflush(stderr);

  // Now switch to ctx1 (which will switch to ctx2, which will switch to ctx3,
  // which will switch back to ctx1)
  fprintf(stderr, "Main switching to ctx1\n");
  fflush(stderr);
  wasix_context_switch(ctx1);

  // Verify execution order: ctx1 -> ctx2 -> ctx3 -> ctx1 (resumed)
  fprintf(stderr, "Back in main. Execution order:");
  for (int i = 0; i < order_idx; i++) {
    fprintf(stderr, " %d", execution_order[i]);
  }
  fprintf(stderr, "\n");
  fflush(stderr);

  assert(order_idx == 4 && "Should have 4 execution steps");
  assert(execution_order[0] == 1 && "ctx1 should run first");
  assert(execution_order[1] == 2 &&
         "ctx2 should run second (BUG: might be ctx3)");
  assert(execution_order[2] == 3 && "ctx3 should run third");
  assert(execution_order[3] == 4 && "ctx1 should resume fourth");

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);
  wasix_context_destroy(ctx3);

  fprintf(stderr, "Test passed - correct contexts were activated!\n");
  return 0;
}
