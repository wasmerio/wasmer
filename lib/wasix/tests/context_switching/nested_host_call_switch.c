// Test switching contexts while nested in the middle of syscalls
// This directly tries to trigger "store context still borrowed" by
// calling wasix_context_switch during operations that hold a store borrow
#include <assert.h>
#include <dirent.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2;
int phase = 0;

void do_directory_operations(void);
void do_file_operations(void);

void context1_fn(void) {
  phase = 1;

  // Do syscalls that might hold store borrows
  do_directory_operations();

  phase = 3;
  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  phase = 2;

  // Do different syscalls while ctx1 might have pending operations
  do_file_operations();

  phase = 4;
  wasix_context_switch(ctx1);
}

void do_directory_operations(void) {
  // Create a directory
  mkdir("/tmp/test_dir", 0755);

  // Open the directory
  DIR *dir = opendir("/tmp");
  if (!dir) {
    perror("opendir");
    return;
  }

  // Read some entries
  struct dirent *entry;
  int count = 0;
  while ((entry = readdir(dir)) != NULL && count < 5) {
    count++;

    // Switch context while directory is open and we're in the middle of reading
    if (count == 2) {
      wasix_context_switch(ctx2);
      // After resuming, continue reading directory
    }
  }

  // Close directory
  closedir(dir);

  // Remove the directory
  rmdir("/tmp/test_dir");
}

void do_file_operations(void) {
  // Open multiple files
  int fd1 = open("/tmp/file1.txt", O_CREAT | O_RDWR, 0644);
  int fd2 = open("/tmp/file2.txt", O_CREAT | O_RDWR, 0644);

  if (fd1 < 0 || fd2 < 0) {
    perror("open");
    if (fd1 >= 0)
      close(fd1);
    if (fd2 >= 0)
      close(fd2);
    return;
  }

  // Write to both
  const char *data = "test data\n";
  write(fd1, data, strlen(data));
  write(fd2, data, strlen(data));

  // Get file stats while files are open
  struct stat st;
  fstat(fd1, &st);

  // Close files
  close(fd1);
  close(fd2);

  // Remove files
  unlink("/tmp/file1.txt");
  unlink("/tmp/file2.txt");
}

int main() {
  int ret;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  // Start execution
  wasix_context_switch(ctx1);

  // Verify phases completed
  assert(phase >= 2 && "Should have executed both contexts");

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Nested host call switch test passed\n");
  return 0;
}
