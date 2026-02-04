#include <assert.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

static volatile sig_atomic_t sig_count = 0;
static volatile sig_atomic_t last_sig = 0;

static void handler(int sig)
{
    sig_count++;
    last_sig = sig;
}

static __wasi_pid_t spawn_child(int exit_code)
{
    char cwd[256];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    char name[512];
    int name_len = snprintf(name, sizeof(name), "%s/proc_signal_child.wasm", cwd);
    assert(name_len > 0 && name_len < (int)sizeof(name));

    char args[256];
    int args_len = snprintf(args, sizeof(args), "%s\nexit=%d\ntimeout=1000", name, exit_code);
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

static void test_signal_self(void)
{
    printf("Test 1: proc_signal to self triggers handler\n");
    sig_count = 0;
    last_sig = 0;

    void (*prev)(int) = signal(SIGUSR1, handler);
    assert(prev != SIG_ERR);

    __wasi_pid_t pid = 0;
    __wasi_errno_t err = __wasi_proc_id(&pid);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_proc_signal(pid, __WASI_SIGNAL_USR1);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(sig_count == 1);
    assert(last_sig == SIGUSR1);
}

static void test_signal_zero(void)
{
    printf("Test 2: proc_signal with signal 0 does not deliver\n");
    sig_count = 0;
    last_sig = 0;

    void (*prev)(int) = signal(SIGUSR1, handler);
    assert(prev != SIG_ERR);

    __wasi_pid_t pid = 0;
    __wasi_errno_t err = __wasi_proc_id(&pid);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_proc_signal(pid, __WASI_SIGNAL_NONE);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(sig_count == 0);
    assert(last_sig == 0);
}

static void test_invalid_signal(void)
{
    printf("Test 3: proc_signal invalid signal returns INVAL\n");
    __wasi_pid_t pid = 0;
    __wasi_errno_t err = __wasi_proc_id(&pid);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_proc_signal(pid, (__wasi_signal_t)0xFF);
    assert(err == __WASI_ERRNO_INVAL);
}

static void test_invalid_pid(void)
{
    printf("Test 4: proc_signal invalid pid returns SRCH\n");
    __wasi_pid_t pid = 0;
    __wasi_errno_t err = __wasi_proc_id(&pid);
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_proc_signal(pid + 1000000, __WASI_SIGNAL_USR1);
    assert(err == __WASI_ERRNO_SRCH);
}

static void test_signal_child(void)
{
    printf("Test 5: proc_signal delivers to child\n");
    __wasi_pid_t child = spawn_child(7);
    usleep(20000); // Give the child time to install its signal handler.

    __wasi_errno_t err = __wasi_proc_signal(child, __WASI_SIGNAL_USR1);
    assert(err == __WASI_ERRNO_SUCCESS);

    join_child(child, 7);
}

int main(void)
{
    test_signal_self();
    test_signal_zero();
    test_invalid_signal();
    test_invalid_pid();
    test_signal_child();

    printf("proc_signal tests completed\n");
    return 0;
}
