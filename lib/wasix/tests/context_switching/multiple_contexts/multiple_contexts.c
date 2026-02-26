#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test creating multiple contexts and switching between them in various orders
// Expected execution order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7

wasix_context_id_t ctx1, ctx2, ctx3;
int execution_order[10];
int order_idx = 0;

void context1_fn(void) {
  execution_order[order_idx++] = 1;
  wasix_context_switch(ctx2);

  execution_order[order_idx++] = 4;
  wasix_context_switch(ctx3);

  execution_order[order_idx++] = 6;
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  execution_order[order_idx++] = 2;
  wasix_context_switch(ctx3);

  execution_order[order_idx++] = 5;
  wasix_context_switch(ctx1);
}

void context3_fn(void) {
  execution_order[order_idx++] = 3;
  wasix_context_switch(ctx1);

  execution_order[order_idx++] = 7;
  wasix_context_switch(ctx2);
}

int main() {
  int ret;

  // Create three contexts
  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  ret = wasix_context_create(&ctx3, context3_fn);
  assert(ret == 0 && "Failed to create context 3");

  // Start the chain by switching to context 1
  // Expected execution order: 1 -> 2 -> 3 -> 4 -> 7 -> 5 -> 6
  // Control flow: main -> ctx1(1) -> ctx2(2) -> ctx3(3) -> ctx1(4) -> ctx3(7)
  // -> ctx2(5) -> ctx1(6) -> main
  wasix_context_switch(ctx1);

  // Verify execution order
  int expected[] = {1, 2, 3, 4, 7, 5, 6};
  assert(order_idx == 7 && "Incorrect number of context switches");
  for (int i = 0; i < 7; i++) {
    assert(execution_order[i] == expected[i] && "Incorrect execution order");
  }

  ret = wasix_context_destroy(ctx2);
  assert(ret == 0 && "Failed to destroy context 2");

  ret = wasix_context_destroy(ctx3);
  assert(ret == 0 && "Failed to destroy context 3");

  return 0;
}
