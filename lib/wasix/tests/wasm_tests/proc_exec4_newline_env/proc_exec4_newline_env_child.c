#include <assert.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char** argv) {
  (void)argc;
  (void)argv;

  const char* v = getenv("NEWLINE_ENV");
  assert(v != NULL);
  assert(strcmp(v, "value\nwith\nnewlines") == 0);
  return 0;
}
