#include <assert.h>
#include <stdio.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

int main(void)
{
    char cwd[256];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    char name[512];
    int name_len = snprintf(name, sizeof(name), "%s/proc_exec3_empty_argv_child.wasm", cwd);
    assert(name_len > 0 && name_len < (int)sizeof(name));

    __wasi_errno_t err = __wasi_proc_exec3(name,
                                           "",
                                           "",
                                           __WASI_BOOL_FALSE,
                                           "");
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(!"proc_exec3 (empty argv) returned");
}
