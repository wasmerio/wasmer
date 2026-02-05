#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

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

static __wasi_pid_t spawn_proc_spawn2(const char *name, const char *args, const char *envs,
                                     const __wasi_proc_spawn_fd_op_t *fd_ops, size_t fd_ops_len,
                                     __wasi_bool_t search_path, const char *path)
{
    __wasi_pid_t pid = 0;
    __wasi_errno_t err = __wasi_proc_spawn2(
        name,
        args,
        envs,
        fd_ops,
        fd_ops_len,
        NULL,
        0,
        search_path,
        path,
        &pid);
    if (err != __WASI_ERRNO_SUCCESS) {
        printf("spawn_proc_spawn2 failed: name=%s err=%d\n", name, (int)err);
        fflush(stdout);
    }
    assert(err == __WASI_ERRNO_SUCCESS);
    return pid;
}

static void test_spawn_and_join(void)
{
    printf("Test 1: spawn child and join exit status\n");
    fflush(stdout);
    char cwd[256];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    char name[512];
    int name_len = snprintf(name, sizeof(name), "%s/proc_join_child.wasm", cwd);
    assert(name_len > 0 && name_len < (int)sizeof(name));

    char args[256];
    int args_len = snprintf(args, sizeof(args), "%s\nexit=%d", name, 9);
    assert(args_len > 0 && args_len < (int)sizeof(args));

    __wasi_pid_t pid = spawn_proc_spawn2(name, args, "", NULL, 0, __WASI_BOOL_FALSE, "");
    join_child(pid, 9);
}

static void test_file_actions_addopen(void)
{
    printf("Test 2: file actions addopen -> child reads FD 10\n");
    fflush(stdout);
    const char *fname = "posix_spawn.test";
    const char *text = "Hello, posix_spawn";

    int fd = open(fname, O_CREAT | O_TRUNC | O_WRONLY, 0644);
    assert(fd >= 0);
    assert(write(fd, text, strlen(text)) == (ssize_t)strlen(text));
    close(fd);

    char cwd[256];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    char name[512];
    int name_len = snprintf(name, sizeof(name), "%s/proc_spawn_file_actions_child.wasm", cwd);
    assert(name_len > 0 && name_len < (int)sizeof(name));

    char args[512];
    int args_len = snprintf(args, sizeof(args), "%s", name);
    assert(args_len > 0 && args_len < (int)sizeof(args));

    __wasi_proc_spawn_fd_op_t op = {0};
    op.cmd = __WASI_PROC_SPAWN_FD_OP_NAME_OPEN;
    op.fd = 10;
    op.path = (uint8_t *)fname;
    op.path_len = strlen(fname);
    op.dirflags = 0;
    op.oflags = 0;
    op.fs_rights_base = __WASI_RIGHTS_FD_READ | __WASI_RIGHTS_FD_SEEK | __WASI_RIGHTS_FD_TELL;
    op.fs_rights_inheriting = op.fs_rights_base;
    op.fdflags = 0;
    op.fdflagsext = 0;

    __wasi_pid_t pid = spawn_proc_spawn2(name, args, "", &op, 1, __WASI_BOOL_FALSE, "");
    join_child(pid, 0);
}

static void test_file_actions_dup2_pipe(void)
{
    printf("Test 3: file actions dup2/close with pipe stdout\n");
    fflush(stdout);
    int fds[2];
    assert(pipe(fds) == 0);

    char cwd[256];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    char name[512];
    int name_len = snprintf(name, sizeof(name), "%s/proc_spawn_stdout_child.wasm", cwd);
    assert(name_len > 0 && name_len < (int)sizeof(name));

    char args[512];
    int args_len = snprintf(args, sizeof(args), "%s", name);
    assert(args_len > 0 && args_len < (int)sizeof(args));

    __wasi_proc_spawn_fd_op_t ops[3];
    memset(ops, 0, sizeof(ops));

    ops[0].cmd = __WASI_PROC_SPAWN_FD_OP_NAME_CLOSE;
    ops[0].fd = fds[0];

    ops[1].cmd = __WASI_PROC_SPAWN_FD_OP_NAME_DUP2;
    ops[1].src_fd = fds[1];
    ops[1].fd = 1;

    ops[2].cmd = __WASI_PROC_SPAWN_FD_OP_NAME_CLOSE;
    ops[2].fd = fds[1];

    __wasi_pid_t pid = spawn_proc_spawn2(name, args, "", ops, 3, __WASI_BOOL_FALSE, "");

    close(fds[1]);
    char buf[16] = {0};
    assert(read(fds[0], buf, sizeof(buf)) == 6);
    close(fds[0]);
    assert(strncmp(buf, "hello\n", 6) == 0);

    join_child(pid, 0);
}

static void test_spawn_self_args_env(char *self_path)
{
    printf("Test 4: spawn self with args/env and spawnp-style search path\n");
    fflush(stdout);
    const char *envs =
        "A=B\nA=B\nA=B\nA=B\nA=B\nA=B\nA=B\nA=B\nA=B\nA=B\n"
        "A=B\nA=B\nA=B\nA=B\nA=B\nA=B\nA=B\nA=B\nA=B";

    char cwd[256];
    assert(getcwd(cwd, sizeof(cwd)) != NULL);

    char self_abs[512];
    int self_abs_len = snprintf(self_abs, sizeof(self_abs), "%s/%s", cwd, self_path);
    assert(self_abs_len > 0 && self_abs_len < (int)sizeof(self_abs));

    char args[1024];
    int args_len = snprintf(
        args,
        sizeof(args),
        "%s\n2\n3\n4\n2\n3\n4\n2\n3\n4\n2\n3\n4\n2\n3\n4\n2\n3\n4",
        self_abs);
    assert(args_len > 0 && args_len < (int)sizeof(args));

    __wasi_pid_t pid = spawn_proc_spawn2(self_abs, args, envs, NULL, 0, __WASI_BOOL_FALSE, "");
    join_child(pid, 0);

    __wasi_pid_t pid2 = spawn_proc_spawn2(self_abs, args, envs, NULL, 0, __WASI_BOOL_TRUE, "");
    join_child(pid2, 0);
}

int main(int argc, char **argv)
{
    if (argc > 1) {
        printf("SPAWNED\n");
        return 0;
    }

    printf("WASIX proc_spawn2 integration tests\n");
    fflush(stdout);
    test_spawn_and_join();
    test_file_actions_addopen();
    test_file_actions_dup2_pipe();
    test_spawn_self_args_env(argv[0]);
    printf("All tests passed!\n");
    fflush(stdout);
    return 0;
}
