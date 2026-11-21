//#ExpectedStdout: ok
//#PrefilledFile: notes:original

#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void) {
  struct stat st;
  char byte;
  int fd = open("target", O_CREAT | O_EXCL | O_WRONLY, 0600);
  assert(fd >= 0);
  assert(write(fd, "X", 1) == 1);
  assert(close(fd) == 0);

  assert(unlink("notes") == 0);
  assert(lstat("notes", &st) == -1 && errno == ENOENT);
  assert(symlink("target", "notes") == 0);
  assert(lstat("notes", &st) == 0 && S_ISLNK(st.st_mode));
  fd = open("notes", O_RDONLY);
  assert(fd >= 0);
  assert(read(fd, &byte, 1) == 1 && byte == 'X');
  assert(close(fd) == 0);
  assert(unlink("notes") == 0);
  assert(lstat("notes", &st) == -1 && errno == ENOENT);
  puts("ok");
  return 0;
}
