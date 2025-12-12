#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wasix/context.h>

// Test context switching while file operations are pending

wasix_context_id_t ctx1, ctx2, ctx3;
int pipe_fds[2];

void context1_fn(void) {
  char buffer[256];

  // Open a file for writing
  FILE *fp = fopen("/tmp/ctx_test1.txt", "w");
  assert(fp != NULL && "Failed to open file");

  // Write some data
  fprintf(fp, "Context 1 writing data\n");
  fflush(fp);

  // Don't close the file yet, switch to another context
  wasix_context_switch(ctx2);

  // After resuming, continue with the file
  fprintf(fp, "Context 1 continued\n");
  fclose(fp);

  // Read from pipe
  ssize_t n = read(pipe_fds[0], buffer, sizeof(buffer));
  assert(n > 0 && "Failed to read from pipe");

  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  char buffer[256];

  // Open another file
  FILE *fp = fopen("/tmp/ctx_test2.txt", "w");
  assert(fp != NULL && "Failed to open file");

  fprintf(fp, "Context 2 data\n");

  // Write to pipe while file is open
  write(pipe_fds[1], "pipe data", 9);

  // Switch with file still open
  wasix_context_switch(ctx3);

  // Resume and close
  fclose(fp);

  wasix_context_switch(ctx1);
}

void context3_fn(void) {
  // Do some operations
  FILE *fp = tmpfile();
  assert(fp != NULL && "Failed to create temp file");

  fprintf(fp, "Temp data\n");
  rewind(fp);

  char buffer[64];
  fgets(buffer, sizeof(buffer), fp);

  // Switch while temp file is open
  wasix_context_switch(ctx2);

  // This code won't execute in this flow
  fclose(fp);
}

int main() {
  int ret;

  // Create pipe
  ret = pipe(pipe_fds);
  assert(ret == 0 && "Failed to create pipe");

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  ret = wasix_context_create(&ctx3, context3_fn);
  assert(ret == 0 && "Failed to create context 3");

  // Start execution
  wasix_context_switch(ctx1);

  // Cleanup
  close(pipe_fds[0]);
  close(pipe_fds[1]);
  unlink("/tmp/ctx_test1.txt");
  unlink("/tmp/ctx_test2.txt");

  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);
  wasix_context_destroy(ctx3);

  fprintf(stderr, "Pending file operations test passed\n");
  return 0;
}
