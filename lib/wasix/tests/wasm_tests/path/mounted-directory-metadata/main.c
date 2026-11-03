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

static int check_entry(const char* path, const char* name, unsigned char type) {
  DIR* directory = opendir(path);
  if (directory == NULL) {
    fprintf(stderr, "opendir(%s): %s\n", path, strerror(errno));
    return 1;
  }
  int found = 0;
  struct dirent* entry;
  while ((entry = readdir(directory)) != NULL) {
    if (strcmp(entry->d_name, name) == 0 && entry->d_type == type) {
      found = 1;
    }
  }
  if (closedir(directory) != 0 || !found) {
    fprintf(stderr, "readdir(%s) did not report %s with type %u\n", path, name,
            type);
    return 1;
  }
  return 0;
}

int main(void) {
  if (check_directory("/mounted/subdir") != 0 ||
      check_entry("/mounted", "subdir", DT_DIR) != 0 ||
      check_entry("/mounted/subdir", "file.txt", DT_REG) != 0) {
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

  FILE* file = fopen("/mounted/subdir/file.txt", "r");
  if (file == NULL) {
    fprintf(stderr, "open nested file: %s\n", strerror(errno));
    return 1;
  }
  char contents[64];
  if (fgets(contents, sizeof(contents), file) == NULL ||
      strcmp(contents, "mounted directory metadata fixture\n") != 0) {
    fprintf(stderr, "nested file did not contain backing contents\n");
    fclose(file);
    return 1;
  }
  if (fclose(file) != 0) return 1;

  puts("mounted directory metadata ok");
  return 0;
}
