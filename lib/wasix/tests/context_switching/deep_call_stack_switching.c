#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test rapid context switching with active function calls on the stack

wasix_context_id_t ctx1, ctx2, ctx3;
int counter = 0;

// Deep call stack with context switches in the middle
void deeply_nested_function_a(int depth);
void deeply_nested_function_b(int depth);
void deeply_nested_function_c(int depth);

void deeply_nested_function_a(int depth) {
  if (depth == 0) {
    counter++;
    wasix_context_switch(ctx2);
    return;
  }

  deeply_nested_function_b(depth - 1);
}

void deeply_nested_function_b(int depth) {
  if (depth == 0) {
    counter++;
    wasix_context_switch(ctx3);
    return;
  }

  deeply_nested_function_c(depth - 1);
}

void deeply_nested_function_c(int depth) {
  if (depth == 0) {
    counter++;
    wasix_context_switch(ctx1);
    return;
  }

  deeply_nested_function_a(depth - 1);
}

void context1_fn(void) {
  deeply_nested_function_a(20);

  // After resuming from the switch and the recursion unwinding
  // We should always have counter >= 3 because all three contexts incremented
  // it
  assert(counter >= 3);
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  deeply_nested_function_b(15);
  // After resuming and recursion unwinding, switch back to continue the chain
  // This shouldn't actually be reached in the current flow
  wasix_context_switch(wasix_context_main);
}

void context3_fn(void) {
  deeply_nested_function_c(10);
  // After resuming and recursion unwinding
  // This shouldn't actually be reached in the current flow
  wasix_context_switch(wasix_context_main);
}

int main() {
  int ret;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  ret = wasix_context_create(&ctx3, context3_fn);
  assert(ret == 0 && "Failed to create context 3");

  // Start execution
  wasix_context_switch(ctx1);

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);
  wasix_context_destroy(ctx3);

  fprintf(stderr, "Deep call stack switching test passed (counter=%d)\n",
          counter);
  return 0;
}
