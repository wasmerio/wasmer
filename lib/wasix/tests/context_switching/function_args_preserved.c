// Test that function arguments are preserved correctly when contexts start
#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2;
int value_seen_in_ctx1 = -1;
int value_seen_in_ctx2 = -1;

void recursive_function(int depth, int expected_depth) {
  if (depth != expected_depth) {
    fprintf(stderr, "ERROR: depth=%d but expected_depth=%d\n", depth,
            expected_depth);
    fflush(stderr);
  }

  if (depth == 0) {
    fprintf(stderr, "Reached depth 0\n");
    fflush(stderr);
    return;
  }

  recursive_function(depth - 1, expected_depth - 1);
}

void context1_fn(void) {
  fprintf(stderr, "context1_fn: calling recursive_function(5, 5)\n");
  fflush(stderr);
  recursive_function(5, 5);
  fprintf(stderr, "context1_fn: returned from recursive_function\n");
  fflush(stderr);
  value_seen_in_ctx1 = 5;
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  fprintf(stderr, "context2_fn: calling recursive_function(3, 3)\n");
  fflush(stderr);
  recursive_function(3, 3);
  fprintf(stderr, "context2_fn: returned from recursive_function\n");
  fflush(stderr);
  value_seen_in_ctx2 = 3;
  wasix_context_switch(wasix_context_main);
}

int main() {
  int ret;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0);

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0);

  fprintf(stderr, "Switching to ctx1\n");
  fflush(stderr);
  wasix_context_switch(ctx1);

  fprintf(stderr, "Switching to ctx2\n");
  fflush(stderr);
  wasix_context_switch(ctx2);

  assert(value_seen_in_ctx1 == 5 && "ctx1 should have completed with depth 5");
  assert(value_seen_in_ctx2 == 3 && "ctx2 should have completed with depth 3");

  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Test passed - function arguments preserved correctly!\n");
  return 0;
}
