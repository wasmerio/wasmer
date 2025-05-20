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

int successful_execlp()
{
    // We should be able to handle an extra / at the end
    putenv("PATH=/home/");

    int pid = vfork();

    if (pid == 0)
    {
        execlp("main.wasm", "main.wasm", "subprocess", NULL);
        perror("execlp");
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

int exiting_child()
{
    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return -1;
    }

    fcntl(pipefd[0], F_SETFD, FD_CLOEXEC);

    int pid = vfork();

    if (pid == 0)
    {
        char fd_buf[5];
        sprintf(fd_buf, "%d", pipefd[1]);
        execl("./main.wasm", "main.wasm", "subprocess_exit", fd_buf, NULL);
        perror("execl");
        return -1;
    }

    close(pipefd[1]);

    int status;
    if (waitpid(pid, &status, 0) < 0)
    {
        perror("waitpid");
        return -1;
    }

    if (WEXITSTATUS(status) != 50)
    {
        printf("exiting_child: expected child to exit with 50, got status %i\n", WEXITSTATUS(status));
        return -1;
    }

    char buf[6];
    int r = read(pipefd[0], buf, 5);
    if (r < 0)
    {
        perror("read");
        return -1;
    }

    buf[r] = 0;
    if (strcmp(buf, "hello"))
    {
        printf("exiting_child: Expected 'hello', got %s\n", buf);
        return -1;
    }

    r = read(pipefd[0], buf, 5);
    if (r != 0)
    {
        printf("exiting_child: Expected pipe to be closed after first read\n");
        return -1;
    }

    return 0;
}

int subprocess_exit(int fd)
{
    if (write(fd, "hello", 5) < 0)
    {
        perror("write");
        return 1;
    }

    // The FD should be closed automatically by the runtime
    return 50;
}

int trapping_child()
{
    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return -1;
    }

    fcntl(pipefd[0], F_SETFD, FD_CLOEXEC);

    int pid = vfork();

    if (pid == 0)
    {
        char fd_buf[5];
        sprintf(fd_buf, "%d", pipefd[1]);
        execl("./main.wasm", "main.wasm", "subprocess_trap", fd_buf, NULL);
        perror("execl");
        return -1;
    }

    close(pipefd[1]);

    char buf[6];
    int r = read(pipefd[0], buf, 5);
    if (r < 0)
    {
        perror("read");
        return -1;
    }

    buf[r] = 0;
    if (strcmp(buf, "hello"))
    {
        printf("trapping_child: Expected 'hello', got %s\n", buf);
        return -1;
    }

    r = read(pipefd[0], buf, 5);
    if (r != 0)
    {
        printf("trapping_child: Expected pipe to be closed after first read\n");
        return -1;
    }

    int status;
    if (waitpid(pid, &status, 0) < 0)
    {
        perror("waitpid");
        return -1;
    }

    if (WEXITSTATUS(status) == 0 || WEXITSTATUS(status) == 1)
    {
        printf("trapping_child: child appears to not have trapped, got status %i\n", WEXITSTATUS(status));
        return -1;
    }

    return 0;
}

int subprocess_trap(int fd)
{
    if (write(fd, "hello", 5) < 0)
    {
        perror("write");
        return 1;
    }

    // A bad function pointer is guaranteed to trap one way or another
    void (*f)(void) = (void (*)(void))0x12345678;
    f();

    return 1;
}

int trap_before_exec()
{
    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return -1;
    }

    int pid = vfork();

    if (pid == 0)
    {
        if (write(pipefd[1], "hello", 5) < 0)
        {
            perror("write");
            return 1;
        }

        // A bad function pointer is guaranteed to trap one way or another
        void (*f)(void) = (void (*)(void))0x12345678;
        f();

        return 100;
    }

    close(pipefd[1]);

    char buf[6];
    int r = read(pipefd[0], buf, 5);
    if (r < 0)
    {
        perror("read");
        return -1;
    }

    buf[r] = 0;
    if (strcmp(buf, "hello"))
    {
        printf("trap_before_exec: Expected 'hello', got %s\n", buf);
        return -1;
    }

    r = read(pipefd[0], buf, 5);
    if (r != 0)
    {
        printf("trap_before_exec: Expected pipe to be closed after first read\n");
        return -1;
    }

    int status;
    if (waitpid(pid, &status, 0) < 0)
    {
        perror("waitpid");
        return -1;
    }

    if (WEXITSTATUS(status) == 0 || WEXITSTATUS(status) == 1)
    {
        printf("trap_before_exec: child appears to not have trapped, got status %i\n", WEXITSTATUS(status));
        return -1;
    }

    return 0;
}

int exit_before_exec()
{
    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return -1;
    }

    int pid = vfork();

    if (pid == 0)
    {
        if (write(pipefd[1], "hello", 5) < 0)
        {
            perror("write");
            return 1;
        }

        return 60;
    }

    close(pipefd[1]);

    char buf[6];
    int r = read(pipefd[0], buf, 5);
    if (r < 0)
    {
        perror("read");
        return -1;
    }

    buf[r] = 0;
    if (strcmp(buf, "hello"))
    {
        printf("exit_before_exec: Expected 'hello', got %s\n", buf);
        return -1;
    }

    r = read(pipefd[0], buf, 5);
    if (r != 0)
    {
        printf("exit_before_exec: Expected pipe to be closed after first read\n");
        return -1;
    }

    int status;
    if (waitpid(pid, &status, 0) < 0)
    {
        perror("waitpid");
        return -1;
    }

    if (WEXITSTATUS(status) != 60)
    {
        printf("exiting_child: expected child to exit with 60, got status %i\n", WEXITSTATUS(status));
        return -1;
    }

    return 0;
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
    if (!strcmp(argv[1], "successful_execlp"))
    {
        return successful_execlp();
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
    else if (!strcmp(argv[1], "exiting_child"))
    {
        return exiting_child();
    }
    else if (!strcmp(argv[1], "subprocess_exit"))
    {
        return subprocess_exit(atoi(argv[2]));
    }
    else if (!strcmp(argv[1], "trapping_child"))
    {
        return trapping_child();
    }
    else if (!strcmp(argv[1], "subprocess_trap"))
    {
        return subprocess_trap(atoi(argv[2]));
    }
    else if (!strcmp(argv[1], "trap_before_exec"))
    {
        return trap_before_exec();
    }
    else if (!strcmp(argv[1], "exit_before_exec"))
    {
        return exit_before_exec();
    }
    else
    {
        printf("bad command %s\n", argv[1]);
        return 1;
    }
}
