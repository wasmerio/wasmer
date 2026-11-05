//! Files through stdio and POSIX calls: fopen/fwrite/fread, stat, rename,
//! access, dup2, fcntl, chdir/getcwd and relative paths, O_APPEND.
//# expect: read: hello, file
//# expect: size: 12 regular: 1
//# expect: renamed: 1 old gone: 1
//# expect: cwd: /tmp/files
//# expect: relative: hello, file
//# expect: dup2: 1 cloexec: 1
//# expect: append: 17
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void) {
  if (mkdir("/tmp/files", 0755) != 0) {
    perror("mkdir");
    return 1;
  }
  FILE* f = fopen("/tmp/files/a.txt", "w");
  if (!f) {
    perror("fopen w");
    return 1;
  }
  fputs("hello, file\n", f);
  fclose(f);

  char buf[64] = {0};
  f = fopen("/tmp/files/a.txt", "r");
  if (!f || !fgets(buf, sizeof buf, f)) {
    perror("fopen r");
    return 1;
  }
  fclose(f);
  printf("read: %s", buf);

  struct stat st;
  if (stat("/tmp/files/a.txt", &st) != 0) {
    perror("stat");
    return 1;
  }
  printf("size: %lld regular: %d\n", (long long)st.st_size,
         S_ISREG(st.st_mode));

  if (rename("/tmp/files/a.txt", "/tmp/files/b.txt") != 0) {
    perror("rename");
    return 1;
  }
  int renamed = access("/tmp/files/b.txt", F_OK) == 0;
  int gone = access("/tmp/files/a.txt", F_OK) != 0 && errno == ENOENT;
  printf("renamed: %d old gone: %d\n", renamed, gone);

  if (chdir("/tmp/files") != 0) {
    perror("chdir");
    return 1;
  }
  char cwd[256];
  printf("cwd: %s\n", getcwd(cwd, sizeof cwd));
  memset(buf, 0, sizeof buf);
  int fd = open("b.txt", O_RDONLY);
  if (fd < 0 || read(fd, buf, sizeof buf - 1) <= 0) {
    perror("relative open");
    return 1;
  }
  printf("relative: %s", buf);

  int copy = dup2(fd, 20);
  int flags = fcntl(fd, F_GETFL);
  fcntl(copy, F_SETFD, FD_CLOEXEC);
  printf("dup2: %d cloexec: %d\n",
         copy == 20 && (flags & O_ACCMODE) == O_RDONLY,
         (fcntl(copy, F_GETFD) & FD_CLOEXEC) != 0);
  close(copy);
  close(fd);

  fd = open("b.txt", O_WRONLY | O_APPEND);
  if (fd < 0 || write(fd, "more\n", 5) != 5) {
    perror("append");
    return 1;
  }
  close(fd);
  stat("b.txt", &st);
  printf("append: %lld\n", (long long)st.st_size);
  unlink("b.txt");
  return 0;
}
