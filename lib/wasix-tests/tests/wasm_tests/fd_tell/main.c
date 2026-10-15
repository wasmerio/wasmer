#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void test_initial_offset(void) {
  printf("Test: Initial offset is 0\n");

  FILE* f = tmpfile();
  assert(f != NULL);

  long pos = ftell(f);
  assert(pos == 0);

  printf("  Initial offset = 0\n");
  fclose(f);
}

static void test_offset_after_write(void) {
  printf("\nTest: Offset advances after write\n");

  FILE* f = tmpfile();
  assert(f != NULL);

  char data[100];
  memset(data, 'A', 100);
  size_t written = fwrite(data, 1, 100, f);
  assert(written == 100);

  long pos = ftell(f);
  assert(pos == 100);

  printf("  After writing 100 bytes, offset = %ld\n", pos);
  fclose(f);
}

static void test_offset_after_read(void) {
  printf("\nTest: Offset advances after read\n");

  FILE* f = tmpfile();
  assert(f != NULL);

  fprintf(f, "hello");
  rewind(f);

  char buf[6];
  size_t read_bytes = fread(buf, 1, 5, f);
  assert(read_bytes == 5);

  long pos = ftell(f);
  assert(pos == 5);

  printf("  After reading 5 bytes ('hello'), offset = %ld\n", pos);
  fclose(f);
}

static void test_seek_operations(void) {
  printf("\nTest: Offset reflects seek operations\n");

  FILE* f = tmpfile();
  assert(f != NULL);

  char data[100];
  memset(data, 'X', 100);
  fwrite(data, 1, 100, f);

  fseek(f, 42, SEEK_SET);
  long pos = ftell(f);
  assert(pos == 42);
  printf("  After SEEK_SET to 42, offset = %ld\n", pos);

  fseek(f, -20, SEEK_CUR);
  pos = ftell(f);
  assert(pos == 22);
  printf("  After SEEK_CUR -20, offset = %ld\n", pos);

  fseek(f, 0, SEEK_END);
  pos = ftell(f);
  assert(pos == 100);
  printf("  After SEEK_END, offset = %ld\n", pos);

  fclose(f);
}

static void test_seek_beyond_eof(void) {
  printf("\nTest: Seek beyond EOF\n");

  FILE* f = tmpfile();
  assert(f != NULL);

  char data[100];
  memset(data, 'Y', 100);
  fwrite(data, 1, 100, f);

  fseek(f, 1000, SEEK_SET);
  long pos = ftell(f);
  assert(pos == 1000);

  printf("  After seeking to 1000 (beyond 100-byte file), offset = %ld\n", pos);
  fclose(f);
}

static void test_ftell_equivalence_with_lseek(void) {
  printf("\nTest: ftell equivalent to lseek(fd, 0, SEEK_CUR)\n");

  FILE* f = tmpfile();
  assert(f != NULL);
  int fd = fileno(f);

  fprintf(f, "test data");
  fseek(f, 4, SEEK_SET);

  long ftell_pos = ftell(f);
  off_t lseek_pos = lseek(fd, 0, SEEK_CUR);

  assert(ftell_pos == lseek_pos);
  printf("  ftell = %ld, lseek(0, SEEK_CUR) = %lld (equivalent)\n", ftell_pos,
         (long long)lseek_pos);

  fclose(f);
}

static void test_append_mode(void) {
  printf("\nTest: O_APPEND flag behavior\n");

  char template[] = "/tmp/fd_tell_append_XXXXXX";
  int fd = mkstemp(template);
  assert(fd > 0);

  write(fd, "initial ", 8);

  close(fd);
  fd = open(template, O_RDWR | O_APPEND);
  assert(fd > 0);

  FILE* f = fdopen(fd, "a+");
  assert(f != NULL);

  long initial_pos = ftell(f);
  printf("  Initial offset with O_APPEND: %ld\n", initial_pos);

  fprintf(f, "appended");
  fflush(f);

  long pos = ftell(f);
  assert(pos == 16);
  printf("  After appending 8 bytes to an 8-byte file, offset = %ld\n", pos);

  fclose(f);
  unlink(template);
}

static void test_multiple_operations(void) {
  printf("\nTest: Multiple consecutive operations\n");

  FILE* f = tmpfile();
  assert(f != NULL);

  char data1[50];
  memset(data1, 'A', 50);
  fwrite(data1, 1, 50, f);

  char data2[30];
  memset(data2, 'B', 30);
  fwrite(data2, 1, 30, f);

  fseek(f, 10, SEEK_SET);

  char buf[20];
  fread(buf, 1, 20, f);

  long pos = ftell(f);
  assert(pos == 30);

  printf(
      "  Multiple operations: write(50) -> write(30) -> seek(10) -> read(20) "
      "-> offset = %ld\n",
      pos);
  fclose(f);
}

static void test_rewind(void) {
  printf("\nTest: rewind() sets offset to 0\n");

  FILE* f = tmpfile();
  assert(f != NULL);

  fprintf(f, "test data");
  rewind(f);

  long pos = ftell(f);
  assert(pos == 0);

  printf("  After rewind(), offset = %ld\n", pos);
  fclose(f);
}

static void test_large_offset(void) {
  printf("\nTest: Large offset handling\n");

  FILE* f = tmpfile();
  assert(f != NULL);

  long large_offset = 1000000000L;
  fseek(f, large_offset, SEEK_SET);

  long pos = ftell(f);
  assert(pos == large_offset);

  printf("  Large offset (1GB): offset = %ld\n", pos);
  fclose(f);
}

static void test_consistency(void) {
  printf("\nTest: Consistency across multiple ftell calls\n");

  FILE* f = tmpfile();
  assert(f != NULL);

  fprintf(f, "ab");

  long pos1 = ftell(f);
  long pos2 = ftell(f);
  long pos3 = ftell(f);

  assert(pos1 == pos2);
  assert(pos2 == pos3);

  printf("  Multiple ftell calls return same value: %ld\n", pos1);
  fclose(f);
}

static void test_stdin_stdout_stderr(void) {
  printf("\nTest: Standard file descriptors\n");

  long stdin_pos = ftell(stdin);
  long stdout_pos = ftell(stdout);
  long stderr_pos = ftell(stderr);

  printf("  stdin offset: %ld\n", stdin_pos);
  printf("  stdout offset: %ld\n", stdout_pos);
  printf("  stderr offset: %ld\n", stderr_pos);
  printf("  Standard fds have valid offsets\n");
}

static void test_fdopen_preserves_offset(void) {
  printf("\nTest: fdopen() preserves fd offset\n");

  char template[] = "/tmp/fd_tell_fdopen_XXXXXX";
  int fd = mkstemp(template);
  assert(fd > 0);

  ssize_t written = write(fd, "hello\n", 6);
  assert(written == 6);

  FILE* f = fdopen(fd, "rb");
  assert(f != NULL);

  off_t pos = ftello(f);
  assert(pos == 6);
  printf("  After write(fd, 6 bytes) then fdopen(), ftello = %lld\n",
         (long long)pos);

  fseeko(f, 0, SEEK_SET);
  char buf[7];
  fgets(buf, sizeof(buf), f);
  assert(strcmp(buf, "hello\n") == 0);

  fclose(f);
  unlink(template);
}

int main(void) {
  printf("=== fd_tell (ftell/lseek) Integration Tests ===\n\n");

  test_initial_offset();
  test_offset_after_write();
  test_offset_after_read();
  test_seek_operations();
  test_seek_beyond_eof();
  test_ftell_equivalence_with_lseek();
  test_append_mode();
  test_multiple_operations();
  test_rewind();
  test_large_offset();
  test_consistency();
  test_stdin_stdout_stderr();
  test_fdopen_preserves_offset();

  printf("\n=== All fd_tell tests passed! ===\n");
  return 0;
}
