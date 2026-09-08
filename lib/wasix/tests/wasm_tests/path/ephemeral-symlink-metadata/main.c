//#MappedDirectory: $temp:/ephemeral
//#CurrentDirectory: /ephemeral
//#ExpectedStdout: ephemeral symlink metadata passed

#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static void assert_symlink_metadata(const struct stat* metadata,
                                    size_t target_length) {
  assert(S_ISLNK(metadata->st_mode));
  assert(metadata->st_ino != 0);
  assert(metadata->st_nlink == 1);
  assert(metadata->st_size == (off_t)target_length);
}

static void unlink_and_assert_missing(const char* path) {
  struct stat metadata;
  assert(unlink(path) == 0);
  errno = 0;
  assert(lstat(path, &metadata) == -1);
  assert(errno == ENOENT);
  errno = 0;
  assert(unlink(path) == -1);
  assert(errno == ENOENT);
}

int main(void) {
  static const char target_name[] = "target.txt";
  static const char target_contents[] = "target-data";
  static const char dangling_target[] = "absent.txt";
  struct stat first;
  struct stat target;
  struct stat metadata;
  char link_target[sizeof(target_name)] = {0};

  int fd = open(target_name, O_CREAT | O_WRONLY | O_TRUNC, 0600);
  assert(fd >= 0);
  assert(write(fd, target_contents, sizeof(target_contents) - 1) ==
         sizeof(target_contents) - 1);
  assert(close(fd) == 0);
  assert(stat(target_name, &target) == 0);
  assert(target.st_ino != 0);

  assert(symlink(target_name, "link") == 0);
  assert(lstat("link", &first) == 0);
  assert_symlink_metadata(&first, sizeof(target_name) - 1);
  assert(first.st_ino != target.st_ino);

  assert(lstat("link", &metadata) == 0);
  assert_symlink_metadata(&metadata, sizeof(target_name) - 1);
  assert(metadata.st_ino == first.st_ino);
  assert(fstatat(AT_FDCWD, "link", &metadata, AT_SYMLINK_NOFOLLOW) == 0);
  assert_symlink_metadata(&metadata, sizeof(target_name) - 1);
  assert(metadata.st_ino == first.st_ino);

  assert(stat("link", &metadata) == 0);
  assert(S_ISREG(metadata.st_mode));
  assert(metadata.st_size == sizeof(target_contents) - 1);
  assert(metadata.st_ino == target.st_ino);

  int directory_fd = open(".", O_RDONLY | O_DIRECTORY);
  assert(directory_fd >= 0);
  assert(fstatat(directory_fd, "link", &metadata, AT_SYMLINK_NOFOLLOW) == 0);
  assert_symlink_metadata(&metadata, sizeof(target_name) - 1);
  assert(metadata.st_ino == first.st_ino);
  assert(fstatat(directory_fd, "link", &metadata, 0) == 0);
  assert(S_ISREG(metadata.st_mode));
  assert(metadata.st_ino == target.st_ino);

  assert(readlink("link", link_target, sizeof(link_target)) ==
         sizeof(target_name) - 1);
  assert(memcmp(link_target, target_name, sizeof(target_name) - 1) == 0);

  assert(symlink(target_name, "second") == 0);
  assert(lstat("second", &metadata) == 0);
  assert_symlink_metadata(&metadata, sizeof(target_name) - 1);
  assert(metadata.st_ino != first.st_ino);
  ino_t second_ino = metadata.st_ino;

  assert(symlink(dangling_target, "dangling") == 0);
  assert(lstat("dangling", &metadata) == 0);
  assert_symlink_metadata(&metadata, sizeof(dangling_target) - 1);
  assert(metadata.st_ino != first.st_ino);
  assert(metadata.st_ino != second_ino);
  ino_t dangling_ino = metadata.st_ino;
  assert(fstatat(directory_fd, "dangling", &metadata, AT_SYMLINK_NOFOLLOW) ==
         0);
  assert_symlink_metadata(&metadata, sizeof(dangling_target) - 1);
  assert(metadata.st_ino == dangling_ino);
  errno = 0;
  assert(fstatat(directory_fd, "dangling", &metadata, 0) == -1);
  assert(errno == ENOENT);
  assert(close(directory_fd) == 0);
  errno = 0;
  assert(stat("dangling", &metadata) == -1);
  assert(errno == ENOENT);
  memset(link_target, 0, sizeof(link_target));
  assert(readlink("dangling", link_target, sizeof(link_target)) ==
         sizeof(dangling_target) - 1);
  assert(memcmp(link_target, dangling_target, sizeof(dangling_target) - 1) ==
         0);

  DIR* directory = opendir(".");
  assert(directory != NULL);
  int found_link = 0;
  struct dirent* entry;
  for (;;) {
    errno = 0;
    entry = readdir(directory);
    if (entry == NULL) {
      assert(errno == 0);
      break;
    }
    if (strcmp(entry->d_name, "link") != 0) {
      continue;
    }
    assert(entry->d_type == DT_LNK || entry->d_type == DT_UNKNOWN);
    assert(lstat(entry->d_name, &metadata) == 0);
    assert_symlink_metadata(&metadata, sizeof(target_name) - 1);
    assert(metadata.st_ino == first.st_ino);
    found_link++;
  }
  assert(closedir(directory) == 0);
  assert(found_link == 1);

  unlink_and_assert_missing("link");
  unlink_and_assert_missing("second");
  unlink_and_assert_missing("dangling");
  assert(stat(target_name, &metadata) == 0);
  assert(S_ISREG(metadata.st_mode));
  assert(metadata.st_size == sizeof(target_contents) - 1);
  assert(metadata.st_ino == target.st_ino);
  fd = open(target_name, O_RDONLY);
  assert(fd >= 0);
  char contents[sizeof(target_contents)] = {0};
  assert(read(fd, contents, sizeof(contents)) == sizeof(target_contents) - 1);
  assert(strcmp(contents, target_contents) == 0);
  assert(close(fd) == 0);

  puts("ephemeral symlink metadata passed");
  return 0;
}
