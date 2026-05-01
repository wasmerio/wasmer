// Minimal: 3 mutually recursive funcs, 3 contexts, circular pattern
#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2, ctx3;

void func_b(int d);
void func_c(int d);

void func_a(int d) {
  fprintf(stderr, "[func_a] d=%d\n", d);
  fflush(stderr);
  if (d == 0) {
    fprintf(stderr, "[func_a] SWITCH to ctx2\n");
    fflush(stderr);
    wasix_context_switch(ctx2);
    return;
  }
  func_b(d - 1);
}

void func_b(int d) {
  fprintf(stderr, "[func_b] d=%d\n", d);
  fflush(stderr);
  if (d == 0) {
    fprintf(stderr, "[func_b] SWITCH to ctx3\n");
    fflush(stderr);
    wasix_context_switch(ctx3);
    return;
  }
  func_c(d - 1);
}

void func_c(int d) {
  fprintf(stderr, "[func_c] d=%d\n", d);
  fflush(stderr);
  if (d == 0) {
    fprintf(stderr, "[func_c] SWITCH to ctx1\n");
    fflush(stderr);
    wasix_context_switch(ctx1);
    return;
  }
  func_a(d - 1);
}

void context1_fn(void) {
  fprintf(stderr, "ctx1_start\n");
  fflush(stderr);
  func_a(5);
  fprintf(stderr, "ctx1_end\n");
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  fprintf(stderr, "ctx2_start\n");
  fflush(stderr);
  func_b(3);
  fprintf(stderr, "ctx2_end\n");
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

void context3_fn(void) {
  fprintf(stderr, "ctx3_start\n");
  fflush(stderr);
  func_c(2);
  fprintf(stderr, "ctx3_end\n");
  fflush(stderr);
  wasix_context_switch(wasix_context_main);
}

int main() {
  wasix_context_create(&ctx1, context1_fn);
  wasix_context_create(&ctx2, context2_fn);
  wasix_context_create(&ctx3, context3_fn);

  fprintf(stderr, "==> ctx1\n");
  fflush(stderr);
  wasix_context_switch(ctx1);

  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);
  wasix_context_destroy(ctx3);
  fprintf(stderr, "PASS\n");
  return 0;
}
