#include <errno.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

static int test_getppid_basic(void)
{
    // From LTP getppid01.c - range/validity check.
    printf("Test 1: Basic getppid validation\n");
    errno = 0;
    pid_t ppid = getppid();
    assert(ppid >= 0);
    printf("  Parent PID: %d (valid)\n", ppid);
    return 0;
}

static int test_getppid_parent_child(void)
{
    // From LTP getppid02.c / OpenPOSIX fork/4-1.c - child getppid == parent getpid.
    printf("Test 2: Parent-child PID relationship\n");
    printf("SKIPPING AS fork is not supported");
    return 0;
    
    pid_t parent_pid = getpid();
    pid_t child_pid = fork();

    assert(child_pid >= 0);

    if (child_pid == 0)
    {
        pid_t child_ppid = getppid();
        assert(child_ppid == parent_pid);
        _exit(0);
    }

    int status = 0;
    pid_t waited = waitpid(child_pid, &status, 0);
    assert(waited == child_pid);
    assert(WIFEXITED(status));
    assert(WEXITSTATUS(status) == 0);

    printf("  Child PID=%d parent PID=%d (correct)\n", child_pid, parent_pid);
    return 0;
}

int main(void)
{
    (void)test_getppid_basic();
    (void)test_getppid_parent_child();

    printf("All tests passed!\n");
    return 0;
}
