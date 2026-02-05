#include <assert.h>
#include <stdio.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

int main(void)
{
    char cwd[256];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    char name[512];
    int name_len = snprintf(name, sizeof(name), "%s/proc_exec2_child.wasm", cwd);
    assert(name_len > 0 && name_len < (int)sizeof(name));

    char args[512];
    int args_len = snprintf(args, sizeof(args), "%s\ncanary", name);
    assert(args_len > 0 && args_len < (int)sizeof(args));

    const char *envs = "LTP_TEST_ENV_VAR=test";

    __wasi_proc_exec2(name, args, envs);
    assert(!"proc_exec2 returned");
}
