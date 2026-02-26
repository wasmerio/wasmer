#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test nested context switches with multiple visits to each context

wasix_context_id_t ctxA, ctxB, ctxC;
int visit_count_A = 0;
int visit_count_B = 0;
int visit_count_C = 0;

void contextA_fn(void) {
  visit_count_A++;

  if (visit_count_A == 1) {
    // First visit: go to B
    wasix_context_switch(ctxB);
    // When we resume here, go to main
    wasix_context_switch(wasix_context_main);
  } else {
    // Second visit: shouldn't happen in this flow
    exit(1);
  }
}

void contextB_fn(void) {
  visit_count_B++;

  if (visit_count_B == 1) {
    // First visit: go to C
    wasix_context_switch(ctxC);
    // When we resume here, go back to A
    wasix_context_switch(ctxA);
  } else {
    // Second visit: shouldn't happen in this flow
    exit(1);
  }
}

void contextC_fn(void) {
  visit_count_C++;

  // Switch to B to resume it
  wasix_context_switch(ctxB);
}

int main() {
  int ret;

  ret = wasix_context_create(&ctxA, contextA_fn);
  assert(ret == 0 && "Failed to create context A");

  ret = wasix_context_create(&ctxB, contextB_fn);
  assert(ret == 0 && "Failed to create context B");

  ret = wasix_context_create(&ctxC, contextC_fn);
  assert(ret == 0 && "Failed to create context C");

  // Start the nested chain
  wasix_context_switch(ctxA);

  // Verify contexts were visited
  // Flow: main -> A(1) -> B(1) -> C(1) -> B -> A -> main
  assert(visit_count_A == 1 && "Context A visited wrong number of times");
  assert(visit_count_B == 1 && "Context B visited wrong number of times");
  assert(visit_count_C == 1 && "Context C visited wrong number of times");

  // Cleanup
  wasix_context_destroy(ctxA);
  wasix_context_destroy(ctxB);
  wasix_context_destroy(ctxC);

  return 0;
}
