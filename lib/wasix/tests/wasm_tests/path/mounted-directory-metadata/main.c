//#Config: control
//#MappedDirectory: mapped:/mounted
//#DefaultMappedDirectories: false
//#ExpectedStdout: mounted directory metadata ok

#include <dirent.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>

static int check_directory(const char* path) {
  struct stat st;

  if (stat(path, &st) != 0) {
    fprintf(stderr, "stat(%s): %s\n", path, strerror(errno));
    return 1;
  }
  if (!S_ISDIR(st.st_mode)) {
    fprintf(stderr, "stat(%s) reported mode %#o\n", path, st.st_mode);
    return 1;
  }
  if (lstat(path, &st) != 0) {
    fprintf(stderr, "lstat(%s): %s\n", path, strerror(errno));
    return 1;
  }
  if (!S_ISDIR(st.st_mode)) {
    fprintf(stderr, "lstat(%s) reported mode %#o\n", path, st.st_mode);
    return 1;
  }

  DIR* directory = opendir(path);
  if (directory == NULL) {
    fprintf(stderr, "opendir(%s): %s\n", path, strerror(errno));
    return 1;
  }
  if (closedir(directory) != 0) {
    fprintf(stderr, "closedir(%s): %s\n", path, strerror(errno));
    return 1;
  }
  return 0;
}

int main(void) {
  if (check_directory("/mounted/subdir") != 0) {
    return 1;
  }

  struct stat st;
  if (stat("/mounted/subdir/file.txt", &st) != 0) {
    fprintf(stderr, "stat nested file: %s\n", strerror(errno));
    return 1;
  }
  if (!S_ISREG(st.st_mode)) {
    fprintf(stderr, "nested file reported mode %#o\n", st.st_mode);
    return 1;
  }

  puts("mounted directory metadata ok");
  return 0;
}
