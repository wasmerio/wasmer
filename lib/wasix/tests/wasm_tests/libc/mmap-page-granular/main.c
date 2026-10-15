//#BuildEnv: WASIXCC_WASM_EXCEPTIONS=no
//#MinimalLibc: v2026-07-03.1
//#ExpectedStdout: ALL TESTS PASSED

// Tests for the page-granular mmap emulation in wasix-libc (WARP-69):
// page-aligned results, zero-fill of fresh and recycled pages, partial
// munmap with region splitting and coalescing, shared file writeback
// via msync/munmap (including fragment offsets after splits), and
// error semantics.
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

#define PAGE 65536ull

static int failures = 0;
#define CHECK(cond, ...)                                      \
  do {                                                        \
    if (!(cond)) {                                            \
      failures++;                                             \
      printf("FAIL %s:%d: %s — ", __FILE__, __LINE__, #cond); \
      printf(__VA_ARGS__);                                    \
      printf("\n");                                           \
    }                                                         \
  } while (0)

static int is_zero(const char* p, size_t n) {
  for (size_t i = 0; i < n; i++)
    if (p[i] != 0) return 0;
  return 1;
}

static void test_anon_basic(void) {
  // Page-aligned, zeroed, usable; sub-page length works.
  char* p =
      mmap(NULL, 100, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
  CHECK(p != MAP_FAILED, "anon mmap failed: %s", strerror(errno));
  CHECK(((uintptr_t)p & (PAGE - 1)) == 0, "not page aligned: %p", (void*)p);
  // Whole page must be accessible and zero, not just 100 bytes.
  CHECK(is_zero(p, PAGE), "fresh anon pages not zero");
  memset(p, 0xAB, PAGE);
  CHECK(munmap(p, 100) == 0, "munmap(len=100) failed: %s", strerror(errno));

  // Recycled pages must read as zero again.
  char* q =
      mmap(NULL, PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
  CHECK(q != MAP_FAILED, "recycle mmap failed");
  CHECK(q == p, "expected recycled address %p, got %p", (void*)p, (void*)q);
  CHECK(is_zero(q, PAGE), "recycled pages not zeroed");
  CHECK(munmap(q, PAGE) == 0, "munmap failed");
}

static void test_partial_munmap(void) {
  // Map 10 pages, unmap 4 in the middle, halves stay usable.
  char* p = mmap(NULL, 10 * PAGE, PROT_READ | PROT_WRITE,
                 MAP_PRIVATE | MAP_ANON, -1, 0);
  CHECK(p != MAP_FAILED, "mmap 10 pages failed");
  memset(p, 0x11, 10 * PAGE);
  CHECK(munmap(p + 3 * PAGE, 4 * PAGE) == 0, "partial munmap failed: %s",
        strerror(errno));
  // Surviving halves keep their contents.
  CHECK(p[0] == 0x11 && p[3 * PAGE - 1] == 0x11, "head lost contents");
  CHECK(p[7 * PAGE] == 0x11 && p[10 * PAGE - 1] == 0x11, "tail lost contents");

  // The hole is reusable and comes back zeroed.
  char* hole = mmap(NULL, 4 * PAGE, PROT_READ | PROT_WRITE,
                    MAP_PRIVATE | MAP_ANON, -1, 0);
  CHECK(hole == p + 3 * PAGE, "expected hole reuse at %p, got %p",
        (void*)(p + 3 * PAGE), (void*)hole);
  CHECK(is_zero(hole, 4 * PAGE), "hole not zeroed on reuse");

  // munmap spanning live + free + live + gap succeeds (Linux semantics).
  CHECK(munmap(p, 10 * PAGE) == 0, "spanning munmap failed");

  // Over-allocate-and-trim (the Zend MM / aligned-alloc pattern).
  size_t align = 4 * PAGE;
  char* big = mmap(NULL, 2 * align, PROT_READ | PROT_WRITE,
                   MAP_PRIVATE | MAP_ANON, -1, 0);
  CHECK(big != MAP_FAILED, "trim-pattern mmap failed");
  uintptr_t aligned = ((uintptr_t)big + align - 1) & ~(align - 1);
  size_t head = aligned - (uintptr_t)big;
  if (head) CHECK(munmap(big, head) == 0, "head trim failed");
  size_t tail = align - head;
  if (tail)
    CHECK(munmap((void*)(aligned + align), tail) == 0, "tail trim failed");
  memset((void*)aligned, 0x22, align);
  CHECK(munmap((void*)aligned, align) == 0, "trimmed munmap failed");
}

static void test_free_reuse_coalesce(void) {
  // Two adjacent freed mappings must coalesce and serve one big one.
  char* a = mmap(NULL, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON,
                 -1, 0);
  char* b = mmap(NULL, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON,
                 -1, 0);
  CHECK(a != MAP_FAILED && b != MAP_FAILED, "setup mmaps failed");
  if (b == a + 2 * PAGE) {
    CHECK(munmap(a, 2 * PAGE) == 0, "munmap a failed");
    CHECK(munmap(b, 2 * PAGE) == 0, "munmap b failed");
    char* c = mmap(NULL, 4 * PAGE, PROT_READ | PROT_WRITE,
                   MAP_PRIVATE | MAP_ANON, -1, 0);
    CHECK(c == a, "coalesced reuse expected %p, got %p", (void*)a, (void*)c);
    CHECK(munmap(c, 4 * PAGE) == 0, "munmap c failed");
  } else {
    munmap(a, 2 * PAGE);
    munmap(b, 2 * PAGE);
  }
}

static void test_file_shared(void) {
  const char* path = "mman_test_file";
  int fd = open(path, O_RDWR | O_CREAT | O_TRUNC, 0644);
  CHECK(fd >= 0, "open failed: %s", strerror(errno));
  char buf[256];
  for (int i = 0; i < 256; i++) buf[i] = (char)i;
  CHECK(write(fd, buf, sizeof buf) == sizeof buf, "write failed");

  // Shared writable mapping: contents visible, msync + munmap write back.
  char* p = mmap(NULL, sizeof buf, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  CHECK(p != MAP_FAILED, "file mmap failed: %s", strerror(errno));
  CHECK(memcmp(p, buf, sizeof buf) == 0, "file contents wrong");
  // Tail past EOF within the page reads as zero.
  CHECK(is_zero(p + sizeof buf, PAGE - sizeof buf), "EOF tail not zero");

  p[0] = 'X';
  CHECK(msync(p, sizeof buf, MS_SYNC) == 0, "msync failed: %s",
        strerror(errno));
  char check;
  CHECK(pread(fd, &check, 1, 0) == 1 && check == 'X',
        "msync did not write back (got %d)", check);

  p[1] = 'Y';
  CHECK(munmap(p, sizeof buf) == 0, "file munmap failed");
  CHECK(pread(fd, &check, 1, 1) == 1 && check == 'Y',
        "munmap did not write back (got %d)", check);

  // Writes past EOF within the page must NOT extend the file.
  off_t sz = lseek(fd, 0, SEEK_END);
  CHECK(sz == sizeof buf, "file size changed: %lld", (long long)sz);

  // MAP_PRIVATE writes must not reach the file.
  char* q = mmap(NULL, sizeof buf, PROT_READ | PROT_WRITE, MAP_PRIVATE, fd, 0);
  CHECK(q != MAP_FAILED, "private file mmap failed");
  q[2] = 'Z';
  CHECK(munmap(q, sizeof buf) == 0, "private munmap failed");
  CHECK(pread(fd, &check, 1, 2) == 1 && check == 2,
        "private write leaked to file (got %d)", check);

  close(fd);
  unlink(path);
}

static void test_file_partial_unmap_writeback(void) {
  const char* path = "mman_test_file2";
  int fd = open(path, O_RDWR | O_CREAT | O_TRUNC, 0644);
  CHECK(fd >= 0, "open failed");
  // 3 pages of 'a'.
  char* fill = malloc(PAGE);
  memset(fill, 'a', PAGE);
  for (int i = 0; i < 3; i++)
    CHECK(write(fd, fill, PAGE) == (ssize_t)PAGE, "write failed");
  free(fill);

  char* p = mmap(NULL, 3 * PAGE, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  CHECK(p != MAP_FAILED, "mmap failed");
  p[0] = '0';         // page 0
  p[PAGE] = '1';      // page 1
  p[2 * PAGE] = '2';  // page 2

  // Unmap the middle page: its bytes are written back, fragments live on.
  CHECK(munmap(p + PAGE, PAGE) == 0, "middle munmap failed");
  char check;
  CHECK(pread(fd, &check, 1, PAGE) == 1 && check == '1',
        "middle page not written back (got %c)", check);

  // Fragments still write back at their correct offsets.
  p[1] = 'H';
  p[2 * PAGE + 1] = 'T';
  CHECK(munmap(p, PAGE) == 0, "head fragment munmap failed");
  CHECK(munmap(p + 2 * PAGE, PAGE) == 0, "tail fragment munmap failed");
  CHECK(pread(fd, &check, 1, 1) == 1 && check == 'H', "head writeback wrong");
  CHECK(pread(fd, &check, 1, 2 * PAGE + 1) == 1 && check == 'T',
        "tail writeback wrong (got %c)", check);

  close(fd);
  unlink(path);
}

static void test_errors(void) {
  // len == 0.
  CHECK(mmap(NULL, 0, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0) ==
                MAP_FAILED &&
            errno == EINVAL,
        "mmap(0) not EINVAL");
  // No sharing flag.
  CHECK(
      mmap(NULL, PAGE, PROT_READ | PROT_WRITE, MAP_ANON, -1, 0) == MAP_FAILED &&
          errno == EINVAL,
      "no-share not EINVAL");
  // Unaligned offset.
  CHECK(mmap(NULL, PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE, 0, 123) ==
                MAP_FAILED &&
            errno == EINVAL,
        "unaligned off not EINVAL");
  // File map with bad fd.
  CHECK(mmap(NULL, PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE, -1, 0) ==
                MAP_FAILED &&
            errno == EBADF,
        "bad fd not EBADF");
  // munmap: unaligned addr, zero len.
  char* p =
      mmap(NULL, PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
  CHECK(p != MAP_FAILED, "setup mmap failed");
  CHECK(munmap(p + 1, PAGE - 1) == -1 && errno == EINVAL,
        "unaligned munmap not EINVAL");
  CHECK(munmap(p, 0) == -1 && errno == EINVAL, "munmap(0) not EINVAL");
  // munmap of never-mapped (page-aligned) range succeeds.
  CHECK(munmap(p, PAGE) == 0, "munmap failed");
  CHECK(munmap(p, PAGE) == 0, "double munmap should succeed");
  // msync over unmapped range is ENOMEM.
  CHECK(msync(p, PAGE, MS_SYNC) == -1 && errno == ENOMEM,
        "msync over free range not ENOMEM");
  // MAP_NORESERVE accepted.
#ifdef MAP_NORESERVE
  char* r = mmap(NULL, PAGE, PROT_READ | PROT_WRITE,
                 MAP_PRIVATE | MAP_ANON | MAP_NORESERVE, -1, 0);
  CHECK(r != MAP_FAILED, "MAP_NORESERVE rejected");
  munmap(r, PAGE);
#endif
}

static void test_large_mapping(void) {
  // A large anonymous mapping (the opcache SHM pattern) must succeed
  // and be usable at both ends. Physical commit is lazy, which cannot
  // be observed from inside; RSS behavior is validated externally.
  size_t big = 128ull << 20;
  char* p =
      mmap(NULL, big, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANON, -1, 0);
  CHECK(p != MAP_FAILED, "big mmap failed: %s", strerror(errno));
  p[0] = 1;
  p[big / 2] = 1;
  p[big - 1] = 1;
  CHECK(munmap(p, big) == 0, "big munmap failed");
}

int main(void) {
  test_anon_basic();
  test_partial_munmap();
  test_free_reuse_coalesce();
  test_file_shared();
  test_file_partial_unmap_writeback();
  test_errors();
  test_large_mapping();
  if (failures == 0)
    printf("ALL TESTS PASSED\n");
  else
    printf("%d FAILURES\n", failures);
  return failures ? 1 : 0;
}
