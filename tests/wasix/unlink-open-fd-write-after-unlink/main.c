#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define PAYLOAD_SIZE 1025

static int write_pattern(FILE* fp, char fill, const char* label) {
  char buffer[PAYLOAD_SIZE];
  memset(buffer, fill, sizeof(buffer));

  size_t n = fwrite(buffer, 1, sizeof(buffer), fp);
  if (n != sizeof(buffer)) {
    fprintf(stderr, "%s short write: wrote %zu bytes\n", label, n);
    return 1;
  }
  if (ferror(fp)) {
    perror(label);
    return 1;
  }
  if (fflush(fp) != 0) {
    perror("fflush");
    return 1;
  }

  return 0;
}

static int verify_fp_pattern(FILE* fp, char fill, const char* label) {
  char expected[PAYLOAD_SIZE];
  char actual[PAYLOAD_SIZE];
  memset(expected, fill, sizeof(expected));

  if (fseek(fp, 0, SEEK_SET) != 0) {
    perror("fseek");
    return 1;
  }

  size_t n = fread(actual, 1, sizeof(actual), fp);
  if (n != sizeof(actual)) {
    fprintf(stderr, "%s short read: read %zu bytes\n", label, n);
    return 1;
  }
  if (memcmp(actual, expected, sizeof(actual)) != 0) {
    fprintf(stderr, "%s verification failed\n", label);
    return 1;
  }

  return 0;
}

static int verify_path_pattern(const char* path, char fill, const char* label) {
  char expected[PAYLOAD_SIZE];
  char actual[PAYLOAD_SIZE];
  memset(expected, fill, sizeof(expected));

  int fd = open(path, O_RDONLY);
  if (fd == -1) {
    perror("open verify path");
    return 1;
  }

  size_t total = 0;
  while (total < sizeof(actual)) {
    ssize_t n = read(fd, actual + total, sizeof(actual) - total);
    if (n < 0) {
      perror("read");
      close(fd);
      return 1;
    }
    if (n == 0) {
      break;
    }
    total += (size_t)n;
  }

  if (close(fd) != 0) {
    perror("close verify path");
    return 1;
  }

  if (total != sizeof(actual)) {
    fprintf(stderr, "%s short path read: read %zu bytes\n", label, total);
    return 1;
  }
  if (memcmp(actual, expected, sizeof(actual)) != 0) {
    fprintf(stderr, "%s path verification failed\n", label);
    return 1;
  }

  return 0;
}

int main() {
  int fd1 = open("/tmp/test.txt", O_CREAT | O_RDWR | O_TRUNC, 0644);
  if (fd1 == -1) {
    perror("open first");
    return 1;
  }
  printf("open succeeded\n");

  if (unlink("/tmp/test.txt") == -1) {
    perror("unlink");
    return 1;
  }
  printf("unlink succeeded\n");

  FILE* first = fdopen(fd1, "w+");
  if (first == NULL) {
    perror("fdopen first");
    return 1;
  }
  printf("fdopen succeeded\n");

  int fd2 = open("/tmp/test.txt", O_CREAT | O_RDWR | O_TRUNC, 0644);
  if (fd2 == -1) {
    perror("open second");
    return 1;
  }

  FILE* second = fdopen(fd2, "w+");
  if (second == NULL) {
    perror("fdopen second");
    return 1;
  }
  printf("recreate succeeded\n");

  if (write_pattern(first, 'a', "first file") != 0) {
    return 1;
  }
  printf("first file write succeeded\n");

  if (write_pattern(second, 'b', "second file") != 0) {
    return 1;
  }
  printf("second file write succeeded\n");

  if (verify_fp_pattern(first, 'a', "first file") != 0) {
    return 1;
  }
  if (verify_path_pattern("/tmp/test.txt", 'b', "second file") != 0) {
    return 1;
  }
  printf("verification succeeded\n");

  if (unlink("/tmp/test.txt") == -1) {
    perror("unlink second");
    return 1;
  }

  if (fclose(second) != 0) {
    perror("fclose second");
    return 1;
  }

  if (fclose(first) != 0) {
    perror("fclose first");
    return 1;
  }

  return 0;
}
