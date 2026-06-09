#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

int main(void) {
  char cwd[256];
  assert(getcwd(cwd, sizeof(cwd)) != NULL);

  char name[512];
  int name_len =
      snprintf(name, sizeof(name), "%s/proc_exec4_newline_arg_child.wasm", cwd);
  assert(name_len > 0 && name_len < (int)sizeof(name));

  const char* argv[] = {name, "line1\nline2"};
  const uint8_t** argv_ptrs = (const uint8_t**)argv;

  __wasi_errno_t err =
      __wasi_proc_exec4(name, argv_ptrs, 2, NULL, 0, __WASI_BOOL_FALSE, "");
  assert(err == __WASI_ERRNO_SUCCESS);
  assert(!"proc_exec4 (newline arg) returned");
}
