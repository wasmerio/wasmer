#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

// Regression test for path_unlink_file on a symlink that exists in the backing
// filesystem but was never created through path_symlink, so it is not cached
// in the parent directory's in-memory entries map. Previously this returned
// EINVAL; it should succeed and remove only the symlink, not its target.
//
// `build.sh` pre-creates `target.txt` and a symlink
// `link-to-target -> target.txt` on the host before this binary runs.
int main(void) {
  const char* target = "target.txt";
  const char* link = "link-to-target";

  struct stat st;
  if (lstat(link, &st) != 0) {
    fprintf(stderr, "lstat(link) before unlink: %s\n", strerror(errno));
    return 1;
  }
  if (!S_ISLNK(st.st_mode)) {
    fprintf(stderr, "link is not a symlink: mode=%o\n", st.st_mode);
    return 1;
  }

  errno = 0;
  if (unlink(link) != 0) {
    fprintf(stderr, "unlink(link): %d (%s)\n", errno, strerror(errno));
    return 1;
  }

  errno = 0;
  if (lstat(link, &st) == 0 || errno != ENOENT) {
    fprintf(stderr, "lstat(link) after unlink should ENOENT, got errno=%d\n",
            errno);
    return 1;
  }

  if (lstat(target, &st) != 0) {
    fprintf(stderr, "target was unexpectedly removed: %s\n", strerror(errno));
    return 1;
  }

  errno = 0;
  if (unlink(link) == 0 || errno != ENOENT) {
    fprintf(stderr,
            "second unlink(link) should fail with ENOENT, got errno=%d (%s)\n",
            errno, strerror(errno));
    return 1;
  }

  printf("0");
  return 0;
}
