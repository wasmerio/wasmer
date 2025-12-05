#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wasix/context.h>

// Test file I/O operations between context switches

wasix_context_id_t ctx1, ctx2;
const char *test_file = "/tmp/context_switch_test.txt";

#define TEST_DATA_1 "Hello from context 1\n"
#define TEST_DATA_2 "Hello from context 2\n"

void context1_fn(void) {
  char buffer[128];
  ssize_t n;
  int fd;

  // Write to file
  fd = open(test_file, O_WRONLY | O_CREAT | O_TRUNC, 0644);
  assert(fd >= 0 && "Failed to open file in context 1");

  n = write(fd, TEST_DATA_1, strlen(TEST_DATA_1));
  assert(n == strlen(TEST_DATA_1) && "Failed to write to file in context 1");
  close(fd);

  // Switch to context 2
  wasix_context_switch(ctx2);

  // After resuming, read what context 2 appended
  fd = open(test_file, O_RDONLY);
  assert(fd >= 0 && "Failed to open file for reading in context 1");

  memset(buffer, 0, sizeof(buffer));
  n = read(fd, buffer, sizeof(buffer) - 1);
  assert(n > 0 && "Failed to read from file in context 1");
  close(fd);

  // Verify both messages are there
  assert(strstr(buffer, TEST_DATA_1) != NULL && "Context 1 data missing");
  assert(strstr(buffer, TEST_DATA_2) != NULL && "Context 2 data missing");

  // Clean up
  unlink(test_file);

  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  char buffer[128];
  ssize_t n;
  int fd;

  // Read what context 1 wrote
  fd = open(test_file, O_RDONLY);
  assert(fd >= 0 && "Failed to open file in context 2");

  memset(buffer, 0, sizeof(buffer));
  n = read(fd, buffer, sizeof(buffer) - 1);
  assert(n > 0 && "Failed to read from file in context 2");
  assert(strcmp(buffer, TEST_DATA_1) == 0 &&
         "Read incorrect data in context 2");
  close(fd);

  // Append to file
  fd = open(test_file, O_WRONLY | O_APPEND);
  assert(fd >= 0 && "Failed to open file for append in context 2");

  n = write(fd, TEST_DATA_2, strlen(TEST_DATA_2));
  assert(n == strlen(TEST_DATA_2) && "Failed to append to file in context 2");
  close(fd);

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

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "File I/O switching test passed\n");
  return 0;
}
