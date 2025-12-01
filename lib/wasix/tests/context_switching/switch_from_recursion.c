// Minimal test: context switches from within a recursive function
#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2;
int call_count = 0;

void recursive_func(int depth) {
  call_count++;
  fprintf(stderr, "[recursive_func depth=%d] call_count=%d\n", depth,
          call_count);
  fflush(stderr);

  if (depth == 0) {
    fprintf(stderr, "[recursive_func] reached depth 0, switching to ctx2\n");
    fflush(stderr);
    wasix_context_switch(ctx2);
    fprintf(stderr, "[recursive_func] resumed after switch\n");
    fflush(stderr);
    return;
  }

  recursive_func(depth - 1);
  fprintf(stderr, "[recursive_func depth=%d] returning\n", depth);
  fflush(stderr);
}

void context1_fn(void) {
  fprintf(stderr, "ctx1: starting\n");
  fflush(stderr);
  recursive_func(3);
  fprintf(stderr, "ctx1: after recursive_func returned\n");
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  fprintf(stderr, "ctx2: starting\n");
  fflush(stderr);
  wasix_context_switch(ctx1);
  fprintf(stderr, "ctx2: ERROR - should not reach here\n");
  fflush(stderr);
}

int main() {
  int ret;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0);

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0);

  fprintf(stderr, "main: switching to ctx1\n");
  fflush(stderr);
  wasix_context_switch(ctx1);

  fprintf(stderr, "main: back from ctx1, call_count=%d\n", call_count);
  fflush(stderr);
  assert(call_count == 4 &&
         "Should have made 4 recursive calls (depth 3,2,1,0)");

  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Test passed!\n");
  return 0;
}
