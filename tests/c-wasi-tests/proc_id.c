#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/types.h>
#include <sys/wait.h>

int main()
{
    // Test 1: Basic getpid - should return valid PID
    // From LTP getpid01.c - validates PID is in valid range
    printf("Test 1: Basic getpid validation\n");
    pid_t pid1 = getpid();
    if (pid1 <= 0)
    {
        fprintf(stderr, "getpid returned invalid PID: %d\n", pid1);
        return 1;
    }
    printf("  PID: %d (valid)\n", pid1);

    // Test 2: Consistency - repeated calls should return same value
    // From stress-ng stress-syscall.c - repeated getpid calls
    printf("Test 2: Consistency across multiple calls\n");
    pid_t pid2 = getpid();
    pid_t pid3 = getpid();

    if (pid1 != pid2 || pid2 != pid3)
    {
        fprintf(stderr, "getpid inconsistent: %d, %d, %d\n", pid1, pid2, pid3);
        return 1;
    }
    printf("  All calls returned same PID: %d\n", pid1);

    // Test 3: Stress test - 1000 calls should all return same value
    // From stress-ng stress-syscall.c - high frequency calls
    printf("Test 3: Stress test (1000 calls)\n");
    for (int i = 0; i < 1000; i++)
    {
        pid_t pid = getpid();
        if (pid != pid1)
        {
            fprintf(stderr, "PID changed on iteration %d: expected %d, got %d\n",
                    i, pid1, pid);
            return 1;
        }
    }
    printf("  All 1000 calls consistent\n");

    // Test 4: Parent-child PID relationship
    // From LTP getpid02.c - fork() returns child PID, getppid() returns parent PID
    printf("Test 4: Parent-child PID relationship\n");
    pid_t parent_pid = getpid();
    pid_t child_pid_from_fork = fork();

    if (child_pid_from_fork < 0)
    {
        fprintf(stderr, "fork failed\n");
        return 1;
    }

    if (child_pid_from_fork == 0)
    {
        // Child process
        pid_t child_own_pid = getpid();
        pid_t parent_pid_from_child = getppid();

        // Verify getppid() in child returns parent's PID
        if (parent_pid_from_child != parent_pid)
        {
            fprintf(stderr, "Child getppid() (%d) != parent getpid() (%d)\n",
                    parent_pid_from_child, parent_pid);
            exit(1);
        }

        // Verify child has different PID than parent
        if (child_own_pid == parent_pid)
        {
            fprintf(stderr, "Child PID (%d) should differ from parent PID (%d)\n",
                    child_own_pid, parent_pid);
            exit(1);
        }

        printf("  Child: my PID=%d, parent PID=%d (correct)\n",
               child_own_pid, parent_pid_from_child);
        exit(0);
    }
    else
    {
        // Parent process
        int status;
        pid_t wait_result = waitpid(child_pid_from_fork, &status, 0);

        if (wait_result < 0)
        {
            fprintf(stderr, "waitpid failed\n");
            return 1;
        }

        if (!WIFEXITED(status))
        {
            fprintf(stderr, "Child did not exit normally\n");
            return 1;
        }

        if (WEXITSTATUS(status) != 0)
        {
            fprintf(stderr, "Child process failed validation\n");
            return 1;
        }

        printf("  Parent: fork returned child PID=%d (correct)\n", child_pid_from_fork);
    }

    printf("All tests passed!\n");
    return 0;
}
