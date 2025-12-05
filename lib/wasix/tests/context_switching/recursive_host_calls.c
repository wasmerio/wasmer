// Test context switching during recursive system calls
// This tries to trigger the store context borrow error by doing
// complex syscalls that might borrow the store while switching
#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2, ctx3;
int recursion_depth = 0;
int max_depth = 5;

void recursive_file_operations(int depth);

void context1_fn(void) {
  recursive_file_operations(0);
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  recursive_file_operations(0);
  wasix_context_switch(ctx3);
  // Should not reach here
  wasix_context_switch(wasix_context_main);
}

void context3_fn(void) {
  recursive_file_operations(0);
  wasix_context_switch(ctx1);
  // Should not reach here
  wasix_context_switch(wasix_context_main);
}

void recursive_file_operations(int depth) {
  if (depth >= max_depth) {
    return;
  }

  recursion_depth = depth;

  // Create a file with dynamic name
  char filename[64];
  snprintf(filename, sizeof(filename), "/tmp/test_%d_%d.txt", (int)getpid(),
           depth);

  // Open file (syscall that borrows store)
  int fd = open(filename, O_CREAT | O_RDWR, 0644);
  if (fd < 0) {
    perror("open");
    return;
  }

  // Write some data (another syscall)
  char data[256];
  snprintf(data, sizeof(data), "Depth: %d, PID: %d\n", depth, (int)getpid());
  ssize_t written = write(fd, data, strlen(data));

  // Switch context while file is open
  if (depth == 2) {
    if (recursion_depth == 2) {
      wasix_context_switch(ctx2);
      // After resuming, continue operations
    }
  }

  // Recurse deeper
  recursive_file_operations(depth + 1);

  // More operations after recursion
  lseek(fd, 0, SEEK_SET);
  char readbuf[256] = {0};
  read(fd, readbuf, sizeof(readbuf) - 1);

  // Close the file
  close(fd);

  // Remove the file (another syscall)
  unlink(filename);
}

int main() {
  int ret;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  ret = wasix_context_create(&ctx3, context3_fn);
  assert(ret == 0 && "Failed to create context 3");

  // Start execution
  wasix_context_switch(ctx1);

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);
  wasix_context_destroy(ctx3);

  fprintf(stderr, "Recursive host calls test passed\n");
  return 0;
}
