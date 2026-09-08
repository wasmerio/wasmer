#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static int fail(const char *operation) {
  fprintf(stderr, "%s failed: errno=%d (%s)\n", operation, errno,
          strerror(errno));
  return 1;
}

static int expect_rmdir_error(const char *path, int expected) {
  errno = 0;
  if (rmdir(path) != -1) {
    fprintf(stderr, "rmdir(%s) unexpectedly succeeded\n", path);
    return 1;
  }
  if (errno != expected) {
    fprintf(stderr, "rmdir(%s) returned errno=%d (%s), expected %d (%s)\n",
            path, errno, strerror(errno), expected, strerror(expected));
    return 1;
  }
  return 0;
}

static int create_file(const char *path) {
  int fd = open(path, O_CREAT | O_EXCL | O_WRONLY, 0600);
  if (fd < 0)
    return fail("open");
  if (write(fd, "x", 1) != 1) {
    close(fd);
    return fail("write");
  }
  if (close(fd) != 0)
    return fail("close");
  return 0;
}

static int stale_cache(void) {
  if (mkdir("/left/stale", 0700) != 0)
    return fail("mkdir stale");
  if (create_file("/right/stale/child") != 0)
    return 1;
  if (unlink("/left/stale/child") != 0)
    return fail("unlink stale child");
  if (rmdir("/right/stale") != 0)
    return fail("rmdir stale");
  return 0;
}

static int uncached_target(void) {
  if (mkdir("/left/uncached", 0700) != 0)
    return fail("mkdir uncached");
  if (rmdir("/right/uncached") != 0)
    return fail("rmdir uncached");
  return 0;
}

static int nonempty(void) {
  struct stat metadata;
  if (mkdir("/left/nonempty", 0700) != 0)
    return fail("mkdir nonempty");
  if (create_file("/left/nonempty/child") != 0)
    return 1;
  if (stat("/right/nonempty", &metadata) != 0)
    return fail("stat nonempty");
  return expect_rmdir_error("/right/nonempty", ENOTEMPTY);
}

static int retry(void) {
  struct stat metadata;
  if (mkdir("/left/retry", 0700) != 0)
    return fail("mkdir retry");
  if (create_file("/left/retry/child") != 0)
    return 1;
  if (stat("/right/retry", &metadata) != 0)
    return fail("stat retry");
  if (expect_rmdir_error("/right/retry", ENOTEMPTY) != 0)
    return 1;
  if (unlink("/left/retry/child") != 0)
    return fail("unlink retry child");
  if (rmdir("/right/retry") != 0)
    return fail("rmdir retry");
  return 0;
}

static int file_target(void) {
  struct stat metadata;
  if (create_file("/left/file") != 0)
    return 1;
  if (stat("/right/file", &metadata) != 0)
    return fail("stat file");
  return expect_rmdir_error("/right/file", ENOTDIR);
}

static int symlink_target(void) {
  struct stat metadata;
  if (mkdir("/left/symlink-target", 0700) != 0)
    return fail("mkdir symlink target");
  if (symlink("symlink-target", "/left/symlink") != 0)
    return fail("symlink");
  if (lstat("/left/symlink", &metadata) != 0)
    return fail("lstat symlink");
  return expect_rmdir_error("/left/symlink", ENOTDIR);
}

static int missing(void) {
  return expect_rmdir_error("/right/missing", ENOENT);
}

int main(int argc, char **argv) {
  if (argc != 2) {
    fprintf(stderr, "expected one case name\n");
    return 1;
  }

  int result;
  if (strcmp(argv[1], "stale-cache") == 0)
    result = stale_cache();
  else if (strcmp(argv[1], "uncached-target") == 0)
    result = uncached_target();
  else if (strcmp(argv[1], "nonempty") == 0)
    result = nonempty();
  else if (strcmp(argv[1], "retry") == 0)
    result = retry();
  else if (strcmp(argv[1], "file") == 0)
    result = file_target();
  else if (strcmp(argv[1], "symlink") == 0)
    result = symlink_target();
  else if (strcmp(argv[1], "missing") == 0)
    result = missing();
  else {
    fprintf(stderr, "unknown case: %s\n", argv[1]);
    return 1;
  }

  if (result != 0)
    return result;
  puts("directory remove case passed");
  return 0;
}
