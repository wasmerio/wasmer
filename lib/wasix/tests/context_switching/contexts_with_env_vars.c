#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wasix/context.h>

// Test context switching with environment variable operations

wasix_context_id_t ctx1, ctx2;

void context1_fn(void) {
  // Set an environment variable
  int ret = setenv("CTX_TEST_VAR", "from_context_1", 1);
  assert(ret == 0 && "Failed to set env var in context 1");

  const char *val = getenv("CTX_TEST_VAR");
  assert(val != NULL && "Env var should exist");
  assert(strcmp(val, "from_context_1") == 0 && "Env var should match");

  // Switch to context 2
  wasix_context_switch(ctx2);

  // After resuming, check if context 2 modified it
  val = getenv("CTX_TEST_VAR");
  assert(val != NULL && "Env var should still exist");
  assert(strcmp(val, "from_context_2") == 0 &&
         "Env var should be modified by context 2");

  // Clean up
  unsetenv("CTX_TEST_VAR");

  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  // Check that we can see the env var set by context 1
  const char *val = getenv("CTX_TEST_VAR");
  assert(val != NULL && "Env var should be visible in context 2");
  assert(strcmp(val, "from_context_1") == 0 &&
         "Env var should have context 1's value");

  // Modify it
  int ret = setenv("CTX_TEST_VAR", "from_context_2", 1);
  assert(ret == 0 && "Failed to modify env var in context 2");

  // Switch back to context 1
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

  // Verify env var was cleaned up
  const char *val = getenv("CTX_TEST_VAR");
  assert(val == NULL && "Env var should be cleaned up");

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Environment variable switching test passed\n");
  return 0;
}
