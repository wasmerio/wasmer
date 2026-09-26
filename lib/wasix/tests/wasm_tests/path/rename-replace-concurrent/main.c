//#ExpectedStdout: ok
// Regression test for path_rename replacing an existing target, in particular
// while other threads rename onto, look up, open and unlink the same name.
// Racing renames used to trip an assertion in path_rename while holding the
// parent directory's inode lock, poisoning it for all later filesystem calls.

#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define THREADS 6
#define ITERATIONS 300

static void write_file(const char* path, const char* contents) {
  int fd = open(path, O_CREAT | O_TRUNC | O_WRONLY, 0644);
  assert(fd >= 0);
  size_t len = strlen(contents);
  assert(write(fd, contents, len) == (ssize_t)len);
  assert(close(fd) == 0);
}

static void read_fd(int fd, char* buf, size_t size) {
  memset(buf, 0, size);
  ssize_t n = pread(fd, buf, size - 1, 0);
  assert(n >= 0);
}

// Replacing a file that is still open must not make later opens of the name
// see the replaced file.
static void replace_open_file(void) {
  char buf[16];
  write_file("replace/target", "old");
  int old_fd = open("replace/target", O_RDONLY);
  assert(old_fd >= 0);
  read_fd(old_fd, buf, sizeof(buf));
  assert(strcmp(buf, "old") == 0);

  write_file("replace/tmp", "new");
  assert(rename("replace/tmp", "replace/target") == 0);

  int new_fd = open("replace/target", O_RDONLY);
  assert(new_fd >= 0);
  read_fd(new_fd, buf, sizeof(buf));
  assert(strcmp(buf, "new") == 0);
  read_fd(old_fd, buf, sizeof(buf));
  assert(strcmp(buf, "old") == 0);

  errno = 0;
  assert(access("replace/tmp", F_OK) == -1 && errno == ENOENT);
  assert(close(old_fd) == 0);
  assert(close(new_fd) == 0);
}

// Renaming onto another link to the same file does nothing.
static void rename_onto_hard_link(void) {
  write_file("link/a", "a");
  assert(link("link/a", "link/b") == 0);
  assert(rename("link/a", "link/b") == 0);
  assert(access("link/a", F_OK) == 0);
  assert(access("link/b", F_OK) == 0);
}

// Renaming a symlink moves the link itself, even if it dangles.
static void rename_dangling_symlink(void) {
  char buf[32] = {0};
  assert(symlink("missing", "symlink/dangling") == 0);
  assert(rename("symlink/dangling", "symlink/moved") == 0);
  assert(readlink("symlink/moved", buf, sizeof(buf) - 1) == 7);
  assert(strcmp(buf, "missing") == 0);
}

// A directory cannot be hard linked; linking one into itself would make the
// directory tree cyclic.
static void link_directory(void) {
  errno = 0;
  assert(link("linkdir", "linkdir/self") == -1);
  assert(errno == EPERM);
  assert(rename("linkdir", "linkdir-moved") == 0);
}

// The threads use absolute paths: libc resolves relative paths against the
// working directory in a buffer that is shared between threads.
static char target[PATH_MAX];

static void* worker(void* arg) {
  int id = (int)(intptr_t)arg;
  char tmp[PATH_MAX];
  char payload[32];
  snprintf(tmp, sizeof(tmp), "%s-tmp-%d", target, id);
  snprintf(payload, sizeof(payload), "payload-%d", id);

  for (int i = 0; i < ITERATIONS; i++) {
    write_file(tmp, payload);
    if (rename(tmp, target) != 0) {
      perror("rename");
      return (void*)1;
    }

    struct stat st;
    if (stat(target, &st) != 0 && errno != ENOENT) {
      perror("stat");
      return (void*)1;
    }

    int fd = open(target, O_RDONLY);
    if (fd >= 0) {
      char buf[32];
      read_fd(fd, buf, sizeof(buf));
      close(fd);
      if (strncmp(buf, "payload-", 8) != 0) {
        fprintf(stderr, "read unexpected contents '%s'\n", buf);
        return (void*)1;
      }
    } else if (errno != ENOENT) {
      perror("open");
      return (void*)1;
    }

    if (i % 5 == 0 && unlink(target) != 0 && errno != ENOENT) {
      perror("unlink");
      return (void*)1;
    }
  }
  return NULL;
}

static void concurrent_replace(void) {
  char cwd[PATH_MAX];
  assert(getcwd(cwd, sizeof(cwd)) != NULL);
  snprintf(target, sizeof(target), "%s/race/target", cwd);

  pthread_t threads[THREADS];
  for (int i = 0; i < THREADS; i++) {
    assert(pthread_create(&threads[i], NULL, worker, (void*)(intptr_t)i) == 0);
  }
  for (int i = 0; i < THREADS; i++) {
    void* result;
    assert(pthread_join(threads[i], &result) == 0);
    assert(result == NULL);
  }

  // Only the target can be left, and at most once.
  DIR* dir = opendir("race");
  assert(dir != NULL);
  int entries = 0;
  struct dirent* entry;
  while ((entry = readdir(dir)) != NULL) {
    if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
      continue;
    }
    assert(strcmp(entry->d_name, "target") == 0);
    entries++;
  }
  assert(closedir(dir) == 0);
  assert(entries <= 1);
}

int main(void) {
  const char* dirs[] = {"replace", "link", "symlink", "linkdir", "race"};
  for (size_t i = 0; i < sizeof(dirs) / sizeof(dirs[0]); i++) {
    assert(mkdir(dirs[i], 0755) == 0);
  }

  replace_open_file();
  rename_onto_hard_link();
  rename_dangling_symlink();
  link_directory();
  concurrent_replace();

  printf("ok\n");
  return 0;
}
