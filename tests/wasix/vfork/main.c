#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
#include <fcntl.h>
#include <errno.h>
#include <sys/wait.h>

int successful_exec()
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

int subprocess()
{
    return 20;
}

int failing_exec()
{
    int pid = vfork();

    if (pid == 0)
    {
        execl("./not-here.wasm", NULL);
        // After the execl fails, this should run and return the correct status
        exit(30);
    }
    else
    {
        int status;
        waitpid(pid, &status, 0);
        if (WEXITSTATUS(status) != 30)
        {
            printf("Expected exit code 30 from subprocess, got %d\n", WEXITSTATUS(status));
            return 1;
        }

        return 0;
    }
}

int cloexec()
{
    int fd = open("/bin/file", O_RDONLY | O_CREAT | O_CLOEXEC);

    int pid = vfork();

    if (pid == 0)
    {
        execl("./not-here.wasm", NULL);

        int flags = fcntl(fd, F_GETFD);
        if (flags == -1)
        {
            perror("fcntl");
            exit(1);
        }

        if ((flags & FD_CLOEXEC) == 0)
        {
            printf("Expected FD_CLOEXEC flag to be set\n");
            exit(2);
        }

        exit(40);
    }
    else
    {
        int status;
        waitpid(pid, &status, 0);
        if (WEXITSTATUS(status) != 40)
        {
            printf("Expected exit code 40 from subprocess, got %d\n", WEXITSTATUS(status));
            return 1;
        }

        return 0;
    }
}

int main(int argc, char **argv)
{
    if (argc < 2)
    {
        return -1;
    }

    if (!strcmp(argv[1], "successful_exec"))
    {
        return successful_exec();
    }
    else if (!strcmp(argv[1], "subprocess"))
    {
        return subprocess();
    }
    else if (!strcmp(argv[1], "failing_exec"))
    {
        return failing_exec();
    }
    else if (!strcmp(argv[1], "cloexec"))
    {
        return cloexec();
    }
    else
    {
        return -1;
    }
}
