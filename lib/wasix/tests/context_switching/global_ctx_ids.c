// Test that global variables containing context IDs are accessible from context
// entrypoints
#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2;

void context1_fn(void) {
  fprintf(stderr, "ctx1 entrypoint: ctx1=%llu, ctx2=%llu\n",
          (unsigned long long)ctx1, (unsigned long long)ctx2);
  fflush(stderr);

  // Try to switch to ctx2
  fprintf(stderr, "ctx1: switching to ctx2\n");
  fflush(stderr);
  wasix_context_switch(ctx2);

  fprintf(stderr, "ctx1: resumed\n");
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  fprintf(stderr, "ctx2 entrypoint: ctx1=%llu, ctx2=%llu\n",
          (unsigned long long)ctx1, (unsigned long long)ctx2);
  fflush(stderr);

  fprintf(stderr, "ctx2: switching back to ctx1\n");
  fflush(stderr);
  wasix_context_switch(ctx1);
}

int main() {
  int ret;

  fprintf(stderr, "Before creation: ctx1=%llu, ctx2=%llu\n",
          (unsigned long long)ctx1, (unsigned long long)ctx2);
  fflush(stderr);

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0);
  fprintf(stderr, "After ctx1 creation: ctx1=%llu\n", (unsigned long long)ctx1);
  fflush(stderr);

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0);
  fprintf(stderr, "After ctx2 creation: ctx2=%llu\n", (unsigned long long)ctx2);
  fflush(stderr);

  fprintf(stderr, "main: switching to ctx1\n");
  fflush(stderr);
  wasix_context_switch(ctx1);

  fprintf(stderr, "Test passed!\n");
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);
  return 0;
}
