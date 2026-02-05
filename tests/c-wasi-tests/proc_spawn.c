#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

static __wasi_pid_t spawn_child(int exit_code)
{
    char cwd[256];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    char name[512];
    int name_len = snprintf(name, sizeof(name), "%s/proc_join_child.wasm", cwd);
    assert(name_len > 0 && name_len < (int)sizeof(name));

    char args[256];
    int args_len = snprintf(args, sizeof(args), "%s\nexit=%d", name, exit_code);
    assert(args_len > 0 && args_len < (int)sizeof(args));

    __wasi_process_handles_t handles;
    __wasi_errno_t err = __wasi_proc_spawn(name,
                                           __WASI_BOOL_FALSE,
                                           args,
                                           "",
                                           __WASI_STDIO_MODE_INHERIT,
                                           __WASI_STDIO_MODE_INHERIT,
                                           __WASI_STDIO_MODE_INHERIT,
                                           cwd,
                                           &handles);
    assert(err == __WASI_ERRNO_SUCCESS);
    return handles.pid;
}

static void join_child(__wasi_pid_t pid, int expected_exit)
{
    __wasi_option_pid_t opt_pid;
    opt_pid.tag = 1;
    opt_pid.u.some = pid;

    __wasi_join_status_t status;
    status.tag = (__wasi_join_status_type_t)0;

    __wasi_errno_t err = __wasi_proc_join(&opt_pid, 0, &status);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(status.tag == __WASI_JOIN_STATUS_TYPE_EXIT_NORMAL);
    assert(status.u.exit_normal == expected_exit);
}

static void test_spawn_and_join(void)
{
    printf("Test 2: spawn child and join exit status\n");
    __wasi_pid_t pid = spawn_child(7);
    join_child(pid, 7);
}

int main(void)
{
    printf("WASIX proc_spawn integration tests\n");
    test_spawn_and_join();
    printf("All tests passed!\n");
    return 0;
}
