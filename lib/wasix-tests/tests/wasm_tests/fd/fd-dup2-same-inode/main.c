//#ExpectedStdout: dup2 same-inode tests passed
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static const char* path = "fd_dup2_same_inode_file";

static void seed_file(void) {
  int fd = open(path, O_CREAT | O_TRUNC | O_RDWR, 0644);
  assert(fd >= 0);
  assert(write(fd, "xy", 2) == 2);
  assert(close(fd) == 0);
}

static void test_dup2_same_inode_replaces_target(void) {
  int fd_a = open(path, O_RDONLY);
  int fd_b = open(path, O_RDONLY);
  assert(fd_a >= 0);
  assert(fd_b >= 0);
  assert(fd_a != fd_b);

  assert(dup2(fd_a, fd_b) == fd_b);

  char value = 0;
  assert(pread(fd_b, &value, 1, 0) == 1);
  assert(value == 'x');

  value = 0;
  assert(pread(fd_a, &value, 1, 1) == 1);
  assert(value == 'y');

  assert(close(fd_a) == 0);
  assert(close(fd_b) == 0);
}

static void test_dup2_same_inode_repeated(void) {
  for (int i = 0; i < 256; i++) {
    int fd_a = open(path, O_RDONLY);
    int fd_b = open(path, O_RDONLY);
    assert(fd_a >= 0);
    assert(fd_b >= 0);

    assert(dup2(fd_a, fd_b) == fd_b);

    char value = 0;
    assert(pread(fd_b, &value, 1, 0) == 1);
    assert(value == 'x');

    assert(close(fd_a) == 0);
    assert(close(fd_b) == 0);
  }
}

static void test_dup2_cross_inode_replaces_target(void) {
  const char* other_path = "fd_dup2_same_inode_other_file";

  unlink(other_path);
  int other_fd = open(other_path, O_CREAT | O_TRUNC | O_RDWR, 0644);
  assert(other_fd >= 0);
  assert(write(other_fd, "z", 1) == 1);
  assert(close(other_fd) == 0);

  int src_fd = open(path, O_RDONLY);
  int dst_fd = open(other_path, O_RDONLY);
  assert(src_fd >= 0);
  assert(dst_fd >= 0);

  assert(dup2(src_fd, dst_fd) == dst_fd);

  char value = 0;
  assert(pread(dst_fd, &value, 1, 0) == 1);
  assert(value == 'x');

  assert(close(src_fd) == 0);
  assert(close(dst_fd) == 0);
  assert(unlink(other_path) == 0);
}

int main(void) {
  unlink(path);
  seed_file();

  test_dup2_same_inode_replaces_target();
  test_dup2_same_inode_repeated();
  test_dup2_cross_inode_replaces_target();

  assert(unlink(path) == 0);
  printf("dup2 same-inode tests passed\n");
  return 0;
}
