#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test creating and destroying multiple contexts in various orders

#define NUM_CONTEXTS 5

wasix_context_id_t contexts[NUM_CONTEXTS];
int execution_flags[NUM_CONTEXTS] = {0};

void context_fn_0(void) {
  execution_flags[0] = 1;
  wasix_context_switch(wasix_context_main);
}

void context_fn_1(void) {
  execution_flags[1] = 1;
  wasix_context_switch(wasix_context_main);
}

void context_fn_2(void) {
  execution_flags[2] = 1;
  wasix_context_switch(wasix_context_main);
}

void context_fn_3(void) {
  execution_flags[3] = 1;
  wasix_context_switch(wasix_context_main);
}

void context_fn_4(void) {
  execution_flags[4] = 1;
  wasix_context_switch(wasix_context_main);
}

int main() {
  int ret;
  void (*entrypoints[])(void) = {context_fn_0, context_fn_1, context_fn_2,
                                 context_fn_3, context_fn_4};

  // Create all contexts
  for (int i = 0; i < NUM_CONTEXTS; i++) {
    ret = wasix_context_create(&contexts[i], entrypoints[i]);
    assert(ret == 0 && "Failed to create context");
  }

  // Execute some contexts
  wasix_context_switch(contexts[0]);
  assert(execution_flags[0] == 1 && "Context 0 should have executed");

  wasix_context_switch(contexts[2]);
  assert(execution_flags[2] == 1 && "Context 2 should have executed");

  wasix_context_switch(contexts[4]);
  assert(execution_flags[4] == 1 && "Context 4 should have executed");

  // Destroy contexts in non-creation order
  ret = wasix_context_destroy(contexts[2]);
  assert(ret == 0 && "Failed to destroy context 2");

  ret = wasix_context_destroy(contexts[0]);
  assert(ret == 0 && "Failed to destroy context 0");

  ret = wasix_context_destroy(contexts[4]);
  assert(ret == 0 && "Failed to destroy context 4");

  // Execute a context that wasn't executed before, then destroy it
  wasix_context_switch(contexts[1]);
  assert(execution_flags[1] == 1 && "Context 1 should have executed");

  ret = wasix_context_destroy(contexts[1]);
  assert(ret == 0 && "Failed to destroy context 1");

  // Destroy the remaining context without ever executing it
  ret = wasix_context_destroy(contexts[3]);
  assert(ret == 0 && "Failed to destroy unexecuted context 3");

  fprintf(stderr, "Context cleanup test passed\n");
  return 0;
}
