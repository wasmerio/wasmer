#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/eventfd.h>
#include <unistd.h>

static int expect_eventfd_read(int fd, uint64_t expected, const char* label) {
  uint64_t out = 0;
  ssize_t nread = read(fd, &out, sizeof(out));

  if (nread != (ssize_t)sizeof(out)) {
    fprintf(stderr, "%s: read failed: nread=%zd errno=%d (%s)\n", label, nread,
            errno, strerror(errno));
    return 1;
  }

  if (out != expected) {
    fprintf(stderr, "%s: expected %" PRIu64 ", got %" PRIu64 "\n", label,
            expected, out);
    return 1;
  }

  return 0;
}

static int test_semaphore_reads_return_one(void) {
  int fd = eventfd(0, EFD_SEMAPHORE | EFD_NONBLOCK);
  if (fd == -1) {
    perror("eventfd");
    return 1;
  }

  uint64_t four = 4;
  if (write(fd, &four, sizeof(four)) != (ssize_t)sizeof(four)) {
    perror("write");
    close(fd);
    return 1;
  }

  for (int i = 0; i < 4; i++) {
    if (expect_eventfd_read(fd, 1, "semaphore read") != 0) {
      close(fd);
      return 1;
    }
  }

  close(fd);
  return 0;
}

int main(void) {
  if (test_semaphore_reads_return_one() != 0) {
    return EXIT_FAILURE;
  }

  puts("ok");
  return EXIT_SUCCESS;
}
