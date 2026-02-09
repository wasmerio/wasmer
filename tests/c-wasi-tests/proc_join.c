#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

static void test_invalid_tag(void)
{
    printf("Test 1: proc_join invalid pid tag\n");
    __wasi_option_pid_t pid;
    pid.tag = 2;
    pid.u.some = 0;

    __wasi_join_status_t status;
    status.tag = (__wasi_join_status_type_t)0xAA;

    __wasi_errno_t err = __wasi_proc_join(&pid, 0, &status);
    assert(err == __WASI_ERRNO_INVAL);
}

static void test_no_children(void)
{
    printf("Test 2: proc_join with no children returns CHILD\n");
    __wasi_option_pid_t pid;
    pid.tag = 0;
    pid.u.none = 0;

    __wasi_join_status_t status;
    status.tag = (__wasi_join_status_type_t)0xAA;

    __wasi_errno_t err = __wasi_proc_join(&pid, 0, &status);
    assert(err == __WASI_ERRNO_CHILD);
    assert(pid.tag == 0);
    assert(status.tag == __WASI_JOIN_STATUS_TYPE_NOTHING);
}

static __wasi_pid_t spawn_child(int exit_code, int sleep_ms)
{
    char cwd[256];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    char name[512];
    int name_len = snprintf(name, sizeof(name), "%s/proc_join_child.wasm", cwd);
    assert(name_len > 0 && name_len < (int)sizeof(name));

    char args[512];
    if (sleep_ms > 0) {
        int args_len = snprintf(args, sizeof(args), "%s\nsleep=%d\nexit=%d", name, sleep_ms, exit_code);
        assert(args_len > 0 && args_len < (int)sizeof(args));
    } else {
        int args_len = snprintf(args, sizeof(args), "%s\nexit=%d", name, exit_code);
        assert(args_len > 0 && args_len < (int)sizeof(args));
    }

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

static void test_join_specific_child(void)
{
    printf("Test 3: proc_join specific child exit status\n");
    __wasi_pid_t child = spawn_child(7, 0);

    __wasi_option_pid_t pid;
    pid.tag = 1;
    pid.u.some = child;

    __wasi_join_status_t status;
    status.tag = (__wasi_join_status_type_t)0xAA;

    __wasi_errno_t err = __wasi_proc_join(&pid, 0, &status);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pid.tag == 1);
    assert(pid.u.some == child);
    assert(status.tag == __WASI_JOIN_STATUS_TYPE_EXIT_NORMAL);
    assert(status.u.exit_normal == 7);
}

static void test_join_any_child(void)
{
    printf("Test 4: proc_join any child\n");
    __wasi_pid_t child = spawn_child(9, 0);

    __wasi_option_pid_t pid;
    pid.tag = 0;
    pid.u.none = 0;

    __wasi_join_status_t status;
    status.tag = (__wasi_join_status_type_t)0xAA;

    __wasi_errno_t err = __wasi_proc_join(&pid, 0, &status);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pid.tag == 1);
    assert(pid.u.some == child);
    assert(status.tag == __WASI_JOIN_STATUS_TYPE_EXIT_NORMAL);
    assert(status.u.exit_normal == 9);
}

static void test_non_blocking_running_child(void)
{
    printf("Test 5: proc_join non-blocking running child returns NOTHING\n");
    __wasi_pid_t child = spawn_child(11, 200);

    __wasi_option_pid_t pid;
    pid.tag = 1;
    pid.u.some = child;

    __wasi_join_status_t status;
    status.tag = (__wasi_join_status_type_t)0xAA;

    __wasi_errno_t err = __wasi_proc_join(&pid, __WASI_JOIN_FLAGS_NON_BLOCKING, &status);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pid.tag == 1);
    assert(pid.u.some == child);
    assert(status.tag == __WASI_JOIN_STATUS_TYPE_NOTHING);

    err = __wasi_proc_join(&pid, 0, &status);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(status.tag == __WASI_JOIN_STATUS_TYPE_EXIT_NORMAL);
    assert(status.u.exit_normal == 11);
}

static void test_non_blocking_missing_pid(void)
{
    printf("Test 6: proc_join non-blocking missing pid returns NOTHING\n");
    __wasi_pid_t self = 0;
    __wasi_errno_t err = __wasi_proc_id(&self);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_option_pid_t pid;
    pid.tag = 1;
    pid.u.some = self + 1000000;

    __wasi_join_status_t status;
    status.tag = (__wasi_join_status_type_t)0xAA;

    err = __wasi_proc_join(&pid, __WASI_JOIN_FLAGS_NON_BLOCKING, &status);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(pid.tag == 0);
    assert(status.tag == __WASI_JOIN_STATUS_TYPE_NOTHING);
}

static void test_bad_status_ptr(void)
{
    printf("Test 7: proc_join invalid status pointer\n");
    __wasi_option_pid_t pid;
    pid.tag = 0;
    pid.u.none = 0;

    __wasi_join_status_t *bad_status = (__wasi_join_status_t *)(uintptr_t)0xFFFFFFFCu;
    __wasi_errno_t err = __wasi_proc_join(&pid, 0, bad_status);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    test_invalid_tag();
    test_no_children();
    test_join_specific_child();
    test_join_any_child();
    test_non_blocking_running_child();
    test_non_blocking_missing_pid();
    test_bad_status_ptr();
    printf("All tests passed!\n");
    return 0;
}
