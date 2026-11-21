//#ExpectedStdout: ok
//#PrefilledFile: occupied:winner

#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void) {
  struct stat st;
  assert(open("occupied", O_CREAT | O_EXCL | O_WRONLY, 0600) == -1);
  assert(errno == EEXIST);
  assert(symlink("target", "link") == 0);

  // The link already exists, so exclusive creation fails with or without
  // O_NOFOLLOW.
  assert(open("link", O_CREAT | O_EXCL | O_WRONLY | O_NOFOLLOW, 0600) == -1);
  assert(errno == EEXIST);
  assert(open("link", O_CREAT | O_EXCL | O_WRONLY, 0600) == -1);
  assert(errno == EEXIST);
  assert(lstat("target", &st) == -1);
  assert(errno == ENOENT);

  // With O_EXCL cleared, O_NOFOLLOW determines whether open creates the target.
  assert(open("link", O_CREAT | O_WRONLY | O_NOFOLLOW, 0600) == -1);
  assert(errno == ELOOP);
  int fd = open("link", O_CREAT | O_WRONLY, 0600);
  assert(fd >= 0);
  assert(close(fd) == 0);
  assert(lstat("target", &st) == 0);
  assert(S_ISREG(st.st_mode));
  assert(mkdir("dir", 0700) == 0);
  assert(symlink("dir", "alias") == 0);
  fd = open("alias/new", O_CREAT | O_EXCL | O_WRONLY | O_NOFOLLOW, 0600);
  assert(fd >= 0);
  assert(close(fd) == 0);
  assert(lstat("dir/new", &st) == 0);
  assert(S_ISREG(st.st_mode));
  printf("ok");
  return 0;
}
