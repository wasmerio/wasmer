// Minimal test to check if the correct entrypoint is being called
#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2, ctx3;

void context1_fn(void) {
  fprintf(stderr, "context1_fn was called!\n");
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  fprintf(stderr, "context2_fn was called!\n");
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

void context3_fn(void) {
  fprintf(stderr, "context3_fn was called!\n");
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

int main() {
  int ret;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0);

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0);

  ret = wasix_context_create(&ctx3, context3_fn);
  assert(ret == 0);

  fprintf(stderr, "Switching to ctx1 (id=%llu)\n", (unsigned long long)ctx1);
  fflush(stderr);
  wasix_context_switch(ctx1);

  fprintf(stderr, "Back in main, switching to ctx2 (id=%llu)\n",
          (unsigned long long)ctx2);
  fflush(stderr);
  wasix_context_switch(ctx2);

  fprintf(stderr, "Back in main, switching to ctx3 (id=%llu)\n",
          (unsigned long long)ctx3);
  fflush(stderr);
  wasix_context_switch(ctx3);

  fprintf(stderr, "Test passed\n");
  return 0;
}
