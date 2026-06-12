#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <sys/stat.h>

int main(void) {
  assert(mkdir("rename-parent", 0755) == 0);

  errno = 0;
  int ret = rename("rename-parent", "rename-parent/child");
  assert(ret == -1);
  assert(errno == EINVAL);
  assert(mkdir("rename-parent/still-here", 0755) == 0);
}
