#include <assert.h>
#include <stdio.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

int main(void)
{
    char cwd[256];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    char name[512];
    int name_len = snprintf(name, sizeof(name), "%s/proc_exec_child.wasm", cwd);
    assert(name_len > 0 && name_len < (int)sizeof(name));

    char args[512];
    int args_len = snprintf(args, sizeof(args), "%s\ncanary", name);
    assert(args_len > 0 && args_len < (int)sizeof(args));

    __wasi_proc_exec(name, args);
    assert(!"proc_exec returned");
}
