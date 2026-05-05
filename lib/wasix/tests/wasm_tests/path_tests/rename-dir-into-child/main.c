#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <sys/stat.h>

int main(void) {
  assert(mkdir("/tmp/rename-parent", 0755) == 0);

  errno = 0;
  int ret = rename("/tmp/rename-parent", "/tmp/rename-parent/child");
  assert(ret == -1);
  assert(errno == EINVAL);
  assert(mkdir("/tmp/rename-parent/still-here", 0755) == 0);
}
