#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wasix/context.h>

// Test context switching with pipe I/O

wasix_context_id_t ctx1, ctx2;
int pipe_fds[2];

#define MSG1 "Message from context 1"
#define MSG2 "Message from context 2"

void context1_fn(void) {
  char buffer[128];

  // Write to pipe
  ssize_t n = write(pipe_fds[1], MSG1, strlen(MSG1));
  assert(n == strlen(MSG1) && "Failed to write to pipe");

  // Switch to context 2
  wasix_context_switch(ctx2);

  // After resuming, read what context 2 wrote
  memset(buffer, 0, sizeof(buffer));
  n = read(pipe_fds[0], buffer, sizeof(buffer) - 1);
  assert(n == strlen(MSG2) && "Failed to read from pipe");
  assert(strcmp(buffer, MSG2) == 0 && "Read incorrect data");

  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  char buffer[128];

  // Read what context 1 wrote
  memset(buffer, 0, sizeof(buffer));
  ssize_t n = read(pipe_fds[0], buffer, sizeof(buffer) - 1);
  assert(n == strlen(MSG1) && "Failed to read from pipe");
  assert(strcmp(buffer, MSG1) == 0 && "Read incorrect data");

  // Write response
  n = write(pipe_fds[1], MSG2, strlen(MSG2));
  assert(n == strlen(MSG2) && "Failed to write to pipe");

  // Switch back to context 1
  wasix_context_switch(ctx1);
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

  // Start execution
  wasix_context_switch(ctx1);

  // Cleanup
  close(pipe_fds[0]);
  close(pipe_fds[1]);
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Pipe I/O switching test passed\n");
  return 0;
}
