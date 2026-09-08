// Stdio whose backing handle has no opinion on being a terminal is reported as
// a character device, so `isatty` says yes. A descriptor duplicated from stdio
// has to give the same answer.

#include <assert.h>
#include <errno.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void) {
  assert(isatty(STDIN_FILENO) == 1);
  assert(isatty(STDOUT_FILENO) == 1);
  assert(isatty(STDERR_FILENO) == 1);

  int dup_of_stdout = dup(STDOUT_FILENO);
  assert(dup_of_stdout >= 0);
  assert(isatty(dup_of_stdout) == 1);
  assert(close(dup_of_stdout) == 0);

  // `fstat` reads the same file type `isatty` does, so the two must agree.
  struct stat st;
  assert(fstat(STDOUT_FILENO, &st) == 0);
  assert(S_ISCHR(st.st_mode));

  return 0;
}
