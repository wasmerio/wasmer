/* Classify GNU make's close_stdout failure; no runtime behavior is replaced.
 * musl src/exit/exit.c:exit deliberately traps recursive exit from atexit.
 * https://wiki.musl-libc.org/functional-differences-from-glibc.html
 */
#include <stdlib.h>

static int direct_exit;

static void handler(void) {
  if (direct_exit) _Exit(7);
  exit(7);
}

int main(int argc, char** argv) {
  (void)argv;
  direct_exit = argc > 1;
  if (atexit(handler)) return 2;
  return 0;
}
