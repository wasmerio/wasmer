#include <stdio.h>
#include <stdlib.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

int main(void) {
  printf("Test 1: Basic getpid validation\n");
  pid_t pid1 = getpid();
  if (pid1 <= 0) {
    fprintf(stderr, "getpid returned invalid PID: %d\n", pid1);
    return 1;
  }
  printf("  PID: %d (valid)\n", pid1);

  printf("Test 2: Consistency across multiple calls\n");
  pid_t pid2 = getpid();
  pid_t pid3 = getpid();

  if (pid1 != pid2 || pid2 != pid3) {
    fprintf(stderr, "getpid inconsistent: %d, %d, %d\n", pid1, pid2, pid3);
    return 1;
  }
  printf("  All calls returned same PID: %d\n", pid1);

  printf("Test 3: Stress test (1000 calls)\n");
  for (int i = 0; i < 1000; i++) {
    pid_t pid = getpid();
    if (pid != pid1) {
      fprintf(stderr, "PID changed on iteration %d: expected %d, got %d\n", i,
              pid1, pid);
      return 1;
    }
  }
  printf("  All 1000 calls consistent\n");

  printf("Test 4: Parent-child PID relationship\n");
  printf("  Skipping: fork is not supported for dynamically linked modules\n");

  printf("All tests passed!\n");
  return 0;
}
