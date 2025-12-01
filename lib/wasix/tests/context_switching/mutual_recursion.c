// Absolute minimal bug: mutually recursive functions with context switch
#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2;

void func_b(int depth);

void func_a(int depth) {
  fprintf(stderr, "[func_a] depth=%d\n", depth);
  fflush(stderr);
  if (depth == 0)
    return;
  func_b(depth - 1);
}

void func_b(int depth) {
  fprintf(stderr, "[func_b] depth=%d\n", depth);
  fflush(stderr);
  if (depth == 0)
    return;
  func_a(depth - 1);
}

void context1_fn(void) {
  fprintf(stderr, "ctx1: calling func_a(3)\n");
  fflush(stderr);
  func_a(3);
  fprintf(stderr, "ctx1: done\n");
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  fprintf(stderr, "ctx2: calling func_b(2)\n");
  fflush(stderr);
  func_b(2);
  fprintf(stderr, "ctx2: done\n");
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

int main() {
  wasix_context_create(&ctx1, context1_fn);
  wasix_context_create(&ctx2, context2_fn);

  fprintf(stderr, "main: switching to ctx1\n");
  fflush(stderr);
  wasix_context_switch(ctx1);

  fprintf(stderr, "main: switching to ctx2\n");
  fflush(stderr);
  wasix_context_switch(ctx2);

  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);
  fprintf(stderr, "Test passed!\n");
  return 0;
}
