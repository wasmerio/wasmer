//! Directories: mkdir, opendir/readdir over enough entries for several
//! fd_readdir rounds, rmdir, symlink/readlink/lstat, and isatty on a pipe.
//# expect: entries: 202 files: 200 dirs: 2
//# expect: readlink: target.txt
//# expect: lstat link: 1 stat file: 1
//# expect: rmdir: ENOTEMPTY then 0
//# expect: isatty(pipe): 0
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define NAME "/tmp/dirs/a-rather-long-file-name-to-fill-the-buffer-%03d"

int main(void) {
  if (mkdir("/tmp/dirs", 0755) != 0) {
    perror("mkdir");
    return 1;
  }
  char path[128];
  for (int i = 0; i < 200; i++) {
    snprintf(path, sizeof path, NAME, i);
    int fd = open(path, O_CREAT | O_WRONLY, 0644);
    if (fd < 0) {
      perror("open");
      return 1;
    }
    close(fd);
  }
  DIR* dir = opendir("/tmp/dirs");
  if (!dir) {
    perror("opendir");
    return 1;
  }
  int entries = 0, files = 0, dirs = 0;
  struct dirent* entry;
  while ((entry = readdir(dir)) != NULL) {
    entries++;
    if (entry->d_type == DT_REG) files++;
    if (entry->d_type == DT_DIR) dirs++;
  }
  closedir(dir);
  printf("entries: %d files: %d dirs: %d\n", entries, files, dirs);

  int fd = open("/tmp/target.txt", O_CREAT | O_WRONLY, 0644);
  close(fd);
  if (symlink("target.txt", "/tmp/link") != 0) {
    perror("symlink");
    return 1;
  }
  char buf[64] = {0};
  ssize_t n = readlink("/tmp/link", buf, sizeof buf - 1);
  printf("readlink: %.*s\n", (int)n, buf);
  struct stat st;
  lstat("/tmp/link", &st);
  int is_link = S_ISLNK(st.st_mode);
  stat("/tmp/link", &st);
  printf("lstat link: %d stat file: %d\n", is_link, S_ISREG(st.st_mode));

  int first = rmdir("/tmp/dirs");
  int err = errno;
  for (int i = 0; i < 200; i++) {
    snprintf(path, sizeof path, NAME, i);
    unlink(path);
  }
  printf("rmdir: %s then %d\n",
         first == -1 && err == ENOTEMPTY ? "ENOTEMPTY" : "?",
         rmdir("/tmp/dirs"));

  int fds[2];
  if (pipe(fds) != 0) {
    perror("pipe");
    return 1;
  }
  printf("isatty(pipe): %d\n", isatty(fds[0]));
  return 0;
}
