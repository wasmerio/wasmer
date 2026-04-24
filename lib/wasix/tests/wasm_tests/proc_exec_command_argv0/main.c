#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#define CHILD_MARKER "child"
#define EXPECTED_MAIN_ARG "--extra-arg"

int main(int argc, char* argv[]) {
  /* Child mode: invoked via execvp(argv[0], ...) from the parent.
   * Verify that the command's main_args are NOT re-injected a second time. */
  for (int i = 1; i < argc; i++) {
    if (strcmp(argv[i], CHILD_MARKER) == 0) {
      if (argc != 2) {
        fprintf(stderr, "FAIL: child expected argc=2 but got %d\n", argc);
        for (int j = 0; j < argc; j++) {
          fprintf(stderr, "  argv[%d] = %s\n", j, argv[j]);
        }
        return 1;
      }
      /* Success: main_args were not re-injected into the re-exec'd process */
      return 0;
    }
  }

  /* Parent mode: verify that the command's main_args were injected for us */
  int found_main_arg = 0;
  for (int i = 1; i < argc; i++) {
    if (strcmp(argv[i], EXPECTED_MAIN_ARG) == 0) {
      found_main_arg = 1;
      break;
    }
  }
  if (!found_main_arg) {
    fprintf(stderr, "FAIL: parent expected '%s' in argv but did not find it\n",
            EXPECTED_MAIN_ARG);
    for (int j = 0; j < argc; j++) {
      fprintf(stderr, "  argv[%d] = %s\n", j, argv[j]);
    }
    return 1;
  }

  /* Re-exec ourselves via argv[0] with the child marker.
   * After the fix, argv[0] is the atom name (not the command name), so
   * execvp will find the raw atom binary without command metadata, preventing
   * main_args from being re-injected. */
  char* new_argv[] = {argv[0], CHILD_MARKER, NULL};
  execvp(argv[0], new_argv);
  perror("execvp failed");
  return 1;
}
