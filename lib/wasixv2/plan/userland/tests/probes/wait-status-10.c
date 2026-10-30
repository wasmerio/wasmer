/* Bash configure's actual fork/wait offset check, with output for cross builds.
 */
#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

int main(void) {
  int status;
  pid_t child = fork();
  if (child < 0) return 1;
  if (child == 0) _exit(42);
  if (wait(&status) != child || !WIFEXITED(status) || WEXITSTATUS(status) != 42)
    return 2;
  for (unsigned int offset = 0; offset < sizeof(status) * 8; ++offset) {
    if (((status >> offset) & 0xff) == 42) {
      printf("%u\n", offset);
      return 0;
    }
  }
  return 3;
}
