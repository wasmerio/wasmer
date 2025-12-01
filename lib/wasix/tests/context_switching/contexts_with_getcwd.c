#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wasix/context.h>

// Test context switching with directory operations

wasix_context_id_t ctx1, ctx2;
char original_dir[1024];

void context1_fn(void) {
  char buf[1024];

  // Get current directory
  char *ret_cwd = getcwd(buf, sizeof(buf));
  assert(ret_cwd != NULL && "Failed to get cwd in context 1");

  // Change to /tmp
  int ret = chdir("/tmp");
  assert(ret == 0 && "Failed to chdir in context 1");

  // Verify the change
  ret_cwd = getcwd(buf, sizeof(buf));
  assert(ret_cwd != NULL && "Failed to get cwd after chdir");
  assert(strcmp(buf, "/tmp") == 0 && "Should be in /tmp");

  // Switch to context 2
  wasix_context_switch(ctx2);

  // After resuming, check if we're still in /tmp
  ret_cwd = getcwd(buf, sizeof(buf));
  assert(ret_cwd != NULL && "Failed to get cwd after resume");
  // Current directory is shared across contexts (same process)
  assert(strcmp(buf, "/") == 0 && "Context 2 should have changed to /");

  // Restore original directory
  ret = chdir(original_dir);
  assert(ret == 0 && "Failed to restore original directory");

  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  char buf[1024];

  // Check current directory (should be /tmp from context 1)
  char *ret_cwd = getcwd(buf, sizeof(buf));
  assert(ret_cwd != NULL && "Failed to get cwd in context 2");
  assert(strcmp(buf, "/tmp") == 0 && "Should be in /tmp from context 1");

  // Change to root
  int ret = chdir("/");
  assert(ret == 0 && "Failed to chdir to / in context 2");

  // Switch back to context 1
  wasix_context_switch(ctx1);
}

int main() {
  int ret;

  // Save original directory
  char *ret_cwd = getcwd(original_dir, sizeof(original_dir));
  assert(ret_cwd != NULL && "Failed to get original cwd");

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  // Start execution
  wasix_context_switch(ctx1);

  // Verify we're back in original directory
  char buf[1024];
  ret_cwd = getcwd(buf, sizeof(buf));
  assert(ret_cwd != NULL && "Failed to get final cwd");
  assert(strcmp(buf, original_dir) == 0 && "Should be back in original dir");

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Directory operations switching test passed\n");
  return 0;
}
