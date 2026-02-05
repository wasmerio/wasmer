#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

int main(void)
{
    char cwd[256];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    const char *name = "proc_exec3_child.wasm";

    char args[256];
    int args_len = snprintf(args, sizeof(args), "%s\ncanary", name);
    assert(args_len > 0 && args_len < (int)sizeof(args));

    __wasi_errno_t err = __wasi_proc_exec3(name,
                                           args,
                                           "",
                                           __WASI_BOOL_TRUE,
                                           cwd);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(!"proc_exec3 (search_path) returned");
}
