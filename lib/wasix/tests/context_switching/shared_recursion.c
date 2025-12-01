// Minimal bug reproduction: calling same recursive functions from different
// contexts
#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2;
int switch_count = 0;

void shared_func(int depth) {
  fprintf(stderr, "[shared_func] depth=%d\n", depth);
  fflush(stderr);

  if (depth == 1 && switch_count == 0) {
    switch_count++;
    fprintf(stderr, "[shared_func] switching to ctx2\n");
    fflush(stderr);
    wasix_context_switch(ctx2);
    fprintf(stderr, "[shared_func] resumed\n");
    fflush(stderr);
  }

  if (depth == 0) {
    return;
  }
  shared_func(depth - 1);
}

void context1_fn(void) {
  fprintf(stderr, "ctx1: calling shared_func(2)\n");
  fflush(stderr);
  shared_func(2);
  fprintf(stderr, "ctx1: done\n");
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  fprintf(stderr, "ctx2: calling shared_func(1)\n");
  fflush(stderr);
  shared_func(1);
  fprintf(stderr, "ctx2: done\n");
  fflush(stderr);
  wasix_context_switch(ctx1);
}

int main() {
  wasix_context_create(&ctx1, context1_fn);
  wasix_context_create(&ctx2, context2_fn);

  fprintf(stderr, "Switching to ctx1\n");
  fflush(stderr);
  wasix_context_switch(ctx1);

  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);
  fprintf(stderr, "Test passed!\n");
  return 0;
}
