#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
#include <fcntl.h>
#include <errno.h>
#include <sys/wait.h>
#include <assert.h>
#include <spawn.h>
#include <wasix/context.h>

wasix_context_id_t ctx1;
int was_in_two = 0;

void context2_fn(void) {
  was_in_two++;
  wasix_context_switch(ctx1);
  assert(0 && "Should not return to context 2");
}

void context1_fn(void) {
  wasix_context_id_t ctx2;
  int ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");
  wasix_context_switch(ctx2);
  wasix_context_destroy(ctx2);
  wasix_context_switch(wasix_context_main);
}

int was_in_context_fn_switch_to_main = 0;
void context_fn_switch_to_main(void) {
  was_in_context_fn_switch_to_main = 1;
  wasix_context_switch(wasix_context_main);
  assert(0);
}

// Test a simple context switching scenario
int test_basic_switching() {
  // Create three contexts
  int ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  wasix_context_switch(ctx1);

  assert(was_in_two == 1 && "Context 2 was not executed exactly once");

  return 0;
}



int vfork_exec()
{
    int pid = vfork();

    if (pid == 0)
    {
        execl("./main.wasm", "main.wasm", "subprocess", NULL);
        perror("execl");
        exit(10);
    }
    else
    {
        int status;
        waitpid(pid, &status, 0);
        if (WEXITSTATUS(status) != 20)
        {
            printf("Expected exit code 20 from subprocess, got %d\n", WEXITSTATUS(status));
            return 1;
        }

        return 0;
    }
}

// Test that vfork works even after other contexts are active
int vfork_after_switching()
{
    wasix_context_id_t ctx;
    int ret = wasix_context_create(&ctx, context_fn_switch_to_main);
    assert(ret == 0 && "Failed to create context");
    wasix_context_switch(ctx);

    int pid = vfork();

    if (pid == 0)
    {
        execl("./main.wasm", "main.wasm", "subprocess", NULL);
        perror("execl");
        exit(10);
    }
    else
    {
        int status;
        waitpid(pid, &status, 0);
        if (WEXITSTATUS(status) != 20)
        {
            printf("Expected exit code 20 from subprocess, got %d\n", WEXITSTATUS(status));
            return 1;
        }

        return 0;
    }
}

// Test that vfork works even after other contexts are active
int vfork_after_switching2()
{
    // Create two contexts. One is to verify switching works now and the other to verify it works later on.
    wasix_context_id_t ctx_a, ctx_b;
    int ret = wasix_context_create(&ctx_a, context_fn_switch_to_main);
    assert(ret == 0 && "Failed to create context");
    ret = wasix_context_create(&ctx_b, context_fn_switch_to_main);
    assert(ret == 0 && "Failed to create context");

    // Verify that switching works before vfork
    was_in_context_fn_switch_to_main = 0;
    wasix_context_switch(ctx_a);
    assert(was_in_context_fn_switch_to_main == 1 && "Context function was not executed");
    
    int pid = vfork();

    if (pid == 0)
    {
        // The process created by vfork should not have a context switching environment
        // so we get a ENOSYS here
        int ret = wasix_context_switch(ctx_b);
        if (ret != -1) {
          exit(11);
        }
        if (errno != ENOTSUP) {
          exit(12);
        }

        execl("./main.wasm", "main.wasm", "subprocess_with_switching", NULL);
        perror("execl");
        exit(10);
    }
    else
    {
        // The parent should still be in the same context switching environment
        was_in_context_fn_switch_to_main = 0;
        wasix_context_switch(ctx_b);
        assert(was_in_context_fn_switch_to_main == 1 && "Context function was not executed");

        int status;
        waitpid(pid, &status, 0);
        if (WEXITSTATUS(status) != 20)
        {
            printf("Expected exit code 20 from subprocess, got %d\n", WEXITSTATUS(status));
            return 1;
        }

        return 0;
    }
}

// Test that vfork works even after other contexts are active
int fork_after_switching()
{
    // Create two contexts. One is to verify switching works now and the other to verify it works later on.
    wasix_context_id_t ctx_a, ctx_b;
    int ret = wasix_context_create(&ctx_a, context_fn_switch_to_main);
    assert(ret == 0 && "Failed to create context");
    ret = wasix_context_create(&ctx_b, context_fn_switch_to_main);
    assert(ret == 0 && "Failed to create context");

    // Verify that switching works before vfork
    was_in_context_fn_switch_to_main = 0;
    wasix_context_switch(ctx_a);
    assert(was_in_context_fn_switch_to_main == 1 && "Context function was not executed");
    
    int pid = fork();

    if (pid == 0)
    {
        // The process created by fork should have a new context switching environment
        // so we get a EINVAL here because the context does not exist in this new environment
        int ret = wasix_context_switch(ctx_b);
        if (ret != -1) {
          exit(11);
        }
        if (errno != EINVAL) {
          exit(12);
        }
        // Recreate the context in the child process
        ret = wasix_context_create(&ctx_b, context_fn_switch_to_main);
        assert(ret == 0 && "Failed to create context");

        // Now we can switch to it. Switching back to main should work too.
        was_in_context_fn_switch_to_main = 0;
        wasix_context_switch(ctx_b);
        assert(was_in_context_fn_switch_to_main == 1 && "Context function was not executed");

        // exec should always bring us in a new context switching environment
        execl("./main.wasm", "main.wasm", "subprocess_with_switching", NULL);
        perror("execl");
        exit(10);
    }
    else
    {
        // The parent should still be in the same context switching environment
        was_in_context_fn_switch_to_main = 0;
        wasix_context_switch(ctx_b);
        assert(was_in_context_fn_switch_to_main == 1 && "Context function was not executed");

        int status;
        waitpid(pid, &status, 0);
        if (WEXITSTATUS(status) != 20)
        {
            printf("Expected exit code 20 from subprocess, got %d\n", WEXITSTATUS(status));
            return 1;
        }

        return 0;
    }
}

void context_fn_that_executes_fork_and_vfork()
{
    int pid = fork();
    assert(pid == -1 && "fork should fail in a context");
    assert(errno == ENOTSUP && "fork should return ENOTSUP in a context");
    pid = vfork();
    assert(pid == -1 && "vfork should fail in a context");
    assert(errno == ENOTSUP && "vfork should return ENOTSUP in a context");
    wasix_context_switch(wasix_context_main);
    assert(0 && "Should not return to this context");
}

int fork_and_vfork_only_work_in_main_context()
{
    wasix_context_id_t ctx;
    int ret = wasix_context_create(&ctx, context_fn_that_executes_fork_and_vfork);
    assert(ret == 0 && "Failed to create context");
    ret = wasix_context_switch(ctx);
    assert(ret == 0 && "Failed to switch to context");
    return 0;
}

extern char **environ;
void context_fn_posix_spawn_a_forking_subprocess_from_a_context()
{
    int pid;
    int status;
    int exit_code;

    // Test a subprocess that does context switching
    char *argV[] = {"./main.wasm", "subprocess_with_switching",(char *) 0};
    posix_spawn(&pid, "./main.wasm", NULL, NULL, argV, environ);
    waitpid(pid, &status, 0);
    exit_code = WEXITSTATUS(status);
    assert(exit_code == 20 && "Expected exit code 20 from subprocess");

    // Test a subprocess that does fork and vfork
    char *argV2[] = {"./main.wasm", "subprocess_with_fork_and_vfork",(char *) 0};
    posix_spawn(&pid, "./main.wasm", NULL, NULL, argV2, environ);
    waitpid(pid, &status, 0);
    exit_code = WEXITSTATUS(status);
    assert(exit_code == 20 && "Expected exit code 20 from subprocess");

    wasix_context_switch(wasix_context_main);
    assert(0 && "Should not return to this context");
}

int posix_spawning_a_forking_subprocess_from_a_context()
{
    wasix_context_id_t ctx;
    int ret = wasix_context_create(&ctx, context_fn_posix_spawn_a_forking_subprocess_from_a_context);
    assert(ret == 0 && "Failed to create context");
    ret = wasix_context_switch(ctx);
    assert(ret == 0 && "Failed to switch to context");
    return 0;
}

int subprocess()
{
    return 20;
}

// Test a simple context switching scenario
int subprocess_with_switching() {
  test_basic_switching();

  return 20;
}

// Test a simple context switching scenario
int subprocess_with_fork_and_vfork() {
  vfork_after_switching2();

  fork_after_switching();

  return 20;
}

int main(int argc, char **argv)
{
    if (argc < 2)
    {
        return -1;
    }


    if (!strcmp(argv[1], "subprocess"))
    {
        return subprocess();
    }
    else if (!strcmp(argv[1], "subprocess_with_switching"))
    {
        return subprocess_with_switching();
    }
        else if (!strcmp(argv[1], "subprocess_with_fork_and_vfork"))
    {
        return subprocess_with_fork_and_vfork();
    }
    else     if (!strcmp(argv[1], "basic_switching"))
    {
        return test_basic_switching();
    }
    else if (!strcmp(argv[1], "vfork_after_switching"))
    {
        return vfork_after_switching();
    }
    else if (!strcmp(argv[1], "vfork_after_switching2"))
    {
        return vfork_after_switching2();
    }
        else if (!strcmp(argv[1], "fork_after_switching"))
    {
        return fork_after_switching();
    }
    else if (!strcmp(argv[1], "fork_and_vfork_only_work_in_main_context"))
    {
        return fork_and_vfork_only_work_in_main_context();
    }
        else if (!strcmp(argv[1], "posix_spawning_a_forking_subprocess_from_a_context"))
    {
        return posix_spawning_a_forking_subprocess_from_a_context();
    }
        else if (!strcmp(argv[1], "fork_and_vfork_only_work_in_main_context2"))
    {
        return fork_and_vfork_only_work_in_main_context();
    }
        else if (!strcmp(argv[1], "fork_and_vfork_only_work_in_main_context2"))
    {
        return fork_and_vfork_only_work_in_main_context();
    }
    else
    {
        printf("bad command %s\n", argv[1]);
        return 1;
    }
}
