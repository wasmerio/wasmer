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
      snprintf(name, sizeof(name), "%s/proc_exec4_newline_env_child.wasm", cwd);
  assert(name_len > 0 && name_len < (int)sizeof(name));

  const char* argv[] = {name};
  const uint8_t** argv_ptrs = (const uint8_t**)argv;

  const char* envp[] = {"NEWLINE_ENV=value\nwith\nnewlines"};
  const uint8_t** envp_ptrs = (const uint8_t**)envp;

  __wasi_errno_t err = __wasi_proc_exec4(name, argv_ptrs, 1, envp_ptrs, 1,
                                         __WASI_BOOL_FALSE, "");
  assert(err == __WASI_ERRNO_SUCCESS);
  assert(!"proc_exec4 (newline env) returned");
}
