#include <assert.h>
#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test context switching with setjmp/longjmp (which manipulates stack state)

wasix_context_id_t ctx1, ctx2;
jmp_buf jump_buffer;
int phase = 0;

void context1_fn(void) {
  int val;

  // Set up a jump point
  val = setjmp(jump_buffer);

  if (val == 0) {
    // First time through
    phase = 1;
    wasix_context_switch(ctx2);

    // After resuming from ctx2
    phase = 3;
    // Try to longjmp after context switch
    longjmp(jump_buffer, 1);
  } else {
    // Jumped back here
    phase = 4;
    wasix_context_switch(wasix_context_main);
  }
}

void context2_fn(void) {
  phase = 2;

  // Do some operations
  char *buf = malloc(1024);
  free(buf);

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

  // Verify execution order
  assert(phase == 4 && "Should have gone through all phases");

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Setjmp/longjmp switching test passed\n");
  return 0;
}
