//#FileSystems: inmemory,tmp
//#MappedDirectory: $temp:/rename

#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static void create_file(const char* path, const char* contents) {
  int fd = open(path, O_CREAT | O_EXCL | O_WRONLY, 0644);
  assert(fd >= 0);
  assert(write(fd, contents, strlen(contents)) == (ssize_t)strlen(contents));
  assert(close(fd) == 0);
}

static void expect_contents(int fd, const char* expected) {
  char contents[32] = {0};
  assert(read(fd, contents, sizeof(contents)) == (ssize_t)strlen(expected));
  assert(strcmp(contents, expected) == 0);
}

int main(void) {
  create_file("/rename/target", "old");
  create_file("/rename/source", "new");

  int replaced = open("/rename/target", O_RDONLY);
  assert(replaced >= 0);
  assert(rename("/rename/source", "/rename/target") == 0);

  // Allocating another file must not reuse the open destination's storage.
  create_file("/rename/other", "other");
  expect_contents(replaced, "old");
  assert(close(replaced) == 0);

  int target = open("/rename/target", O_RDONLY);
  assert(target >= 0);
  expect_contents(target, "new");
  assert(close(target) == 0);

  int other = open("/rename/other", O_RDONLY);
  assert(other >= 0);
  expect_contents(other, "other");
  assert(close(other) == 0);

  assert(unlink("/rename/target") == 0);
  assert(unlink("/rename/other") == 0);
  return 0;
}
