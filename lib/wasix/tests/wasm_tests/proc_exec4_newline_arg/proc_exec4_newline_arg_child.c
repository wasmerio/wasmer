#include <assert.h>
#include <stdio.h>
#include <string.h>

int main(int argc, char** argv) {
  assert(argc >= 2);
  assert(strcmp(argv[1], "line1\nline2") == 0);
  printf("proc_exec4 newline arg test passed\n");
  return 0;
}
