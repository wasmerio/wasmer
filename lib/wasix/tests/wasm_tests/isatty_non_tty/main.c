//#StdioIsTerminal: false

// When the embedder says stdio is not a terminal, `isatty` has to say so, and
// `fstat` has to agree rather than keep claiming a character device.

#include <assert.h>
#include <errno.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void) {
  assert(isatty(STDIN_FILENO) == 0);
  assert(errno == ENOTTY);
  assert(isatty(STDOUT_FILENO) == 0);
  assert(isatty(STDERR_FILENO) == 0);

  int dup_of_stdout = dup(STDOUT_FILENO);
  assert(dup_of_stdout >= 0);
  assert(isatty(dup_of_stdout) == 0);
  assert(close(dup_of_stdout) == 0);

  struct stat st;
  assert(fstat(STDOUT_FILENO, &st) == 0);
  assert(!S_ISCHR(st.st_mode));

  return 0;
}
