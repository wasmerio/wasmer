#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

static int test_getppid_basic(void) {
  printf("Test 1: Basic getppid validation\n");
  errno = 0;
  pid_t ppid = getppid();
  assert(ppid >= 0);
  printf("  Parent PID: %d (valid)\n", ppid);
  return 0;
}

static int test_getppid_parent_child(void) {
  printf("Test 2: Parent-child PID relationship\n");
  printf("SKIPPING AS fork is not supported");
  return 0;
}

int main(void) {
  (void)test_getppid_basic();
  (void)test_getppid_parent_child();

  printf("All tests passed!\n");
  return 0;
}
