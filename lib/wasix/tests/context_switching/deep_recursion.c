#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test that contexts have independent stacks with deep recursion

#define RECURSION_DEPTH 100

wasix_context_id_t ctx1, ctx2;
int ctx1_depth = 0;
int ctx2_depth = 0;
int ctx1_max_depth = 0;
int ctx2_max_depth = 0;

void ctx1_recursive(int depth);
void ctx2_recursive(int depth);

void ctx1_recursive(int depth) {
  ctx1_depth = depth;
  if (depth > ctx1_max_depth) {
    ctx1_max_depth = depth;
  }

  if (depth < RECURSION_DEPTH) {
    // Recurse deeper
    ctx1_recursive(depth + 1);
  } else {
    // Switch to context 2 at max depth
    wasix_context_switch(ctx2);
  }
}

void ctx2_recursive(int depth) {
  ctx2_depth = depth;
  if (depth > ctx2_max_depth) {
    ctx2_max_depth = depth;
  }

  if (depth < RECURSION_DEPTH) {
    // Recurse deeper
    ctx2_recursive(depth + 1);
  } else {
    // Switch back to context 1 at max depth
    wasix_context_switch(ctx1);
  }
}

void context1_fn(void) {
  // Start recursion in context 1
  ctx1_recursive(0);

  // Verify we completed the full recursion
  assert(ctx1_max_depth == RECURSION_DEPTH &&
         "Context 1 should have reached max depth");

  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  // Start recursion in context 2
  ctx2_recursive(0);

  // Verify we completed the full recursion
  assert(ctx2_max_depth == RECURSION_DEPTH &&
         "Context 2 should have reached max depth");

  wasix_context_switch(wasix_context_main);
}

int main() {
  int ret;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  // Start the recursion chain
  wasix_context_switch(ctx1);

  // Verify both contexts reached max depth independently
  assert(ctx1_max_depth == RECURSION_DEPTH && "Context 1 max depth incorrect");
  assert(ctx2_max_depth == RECURSION_DEPTH && "Context 2 max depth incorrect");

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Deep recursion test passed (depth=%d)\n", RECURSION_DEPTH);
  return 0;
}
