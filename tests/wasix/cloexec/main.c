#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <sys/wait.h>

int expect_cloexec_flag(int fd, int expected)
{
    int flags = fcntl(fd, F_GETFD);
    if (flags == -1)
    {
        perror("fcntl in expect_cloexec_flag");
        return -1;
    }

    if ((flags & FD_CLOEXEC) != expected)
    {
        printf("Expected FD_CLOEXEC to be %d, but got %d\n", expected, flags & FD_CLOEXEC);
        return -1;
    }

    return 0;
}

int flag_tests()
{
    int fd = open("/bin/file", O_RDONLY | O_CREAT);
    // WASIX preopens FDs 0 through 5, so fd should be 6
    if (fd != 6)
    {
        perror("open");
        return -1;
    }

    int fd2 = fcntl(fd, F_DUPFD, 4);
    if (fd2 != 7)
    {
        perror("fcntl");
        return -1;
    }

    int fd3 = fcntl(fd, F_DUPFD_CLOEXEC, 10);
    if (fd3 != 10)
    {
        perror("fctnl 2");
        return -1;
    }

    if (expect_cloexec_flag(fd, 0) != 0)
    {
        return -1;
    }

    if (expect_cloexec_flag(fd2, 0) != 0)
    {
        return -1;
    }

    if (expect_cloexec_flag(fd3, FD_CLOEXEC) != 0)
    {
        return -1;
    }

    if (fcntl(fd, F_SETFD, FD_CLOEXEC) != 0)
    {
        perror("fcntl 3");
        return -1;
    }

    if (expect_cloexec_flag(fd, FD_CLOEXEC) != 0)
    {
        return -1;
    }

    int fd4 = open("/bin/file2", O_RDONLY | O_CREAT | O_CLOEXEC);
    if (expect_cloexec_flag(fd4, FD_CLOEXEC) != 0)
    {
        return -1;
    }

    return 0;
}

int exec_tests()
{
    int fd = open("/bin/file", O_RDONLY | O_CREAT | O_CLOEXEC);
    if (fd != 6)
    {
        perror("open");
        return -1;
    }

    int fd2 = fcntl(fd, F_DUPFD, 0);
    if (fd2 != 7)
    {
        perror("fcntl");
        return -1;
    }

    int pid = fork();

    if (pid == -1)
    {
        perror("fork");
        return -1;
    }
    else if (pid == 0)
    {
        execle("./main.wasm", "main.wasm", "exec_subprocess", NULL);
        perror("execle");
        return -1;
    }
    else
    {
        int status;
        if (waitpid(pid, &status, 0) < 0)
        {
            perror("waitpid");
            return -1;
        }

        if (WIFEXITED(status) && WEXITSTATUS(status) != 0)
        {
            printf("Bad status from child process: %d\n", WEXITSTATUS(status));
            return -1;
        }
    }

    return 0;
}

// Since we don't pipe stderr from the child process, this function writes
// output to a file which can (hopefully!) be inspected
void write_subprocess_error(const char *msg)
{
    FILE *outf = fopen("./output.child", "w");
    if (!outf)
    {
        exit(EXIT_FAILURE);
    }
    fprintf(outf, "%s: %s\n", msg, strerror(errno));
    fclose(outf);
}

int exec_subprocess()
{
    int flags = fcntl(6, F_GETFD);
    if (flags != -1 || errno != EBADF)
    {
        write_subprocess_error("Expected EBADF for fd 6");
        return 2;
    }

    flags = fcntl(7, F_GETFD);
    if (flags == -1)
    {
        write_subprocess_error("Error from fcntl in subprocess");
        return 3;
    }

    if ((flags & FD_CLOEXEC) != 0)
    {
        write_subprocess_error("Expected FD_CLOEXEC to be 0 for fd 7");
        return 4;
    }

    return 0;
}

int main(int argc, char **argv)
{
    if (argc < 2)
    {
        return -1;
    }

    if (!strcmp(argv[1], "flag_tests"))
    {
        return flag_tests();
    }
    else if (!strcmp(argv[1], "exec_tests"))
    {
        return exec_tests();
    }
    else if (!strcmp(argv[1], "exec_subprocess"))
    {
        return exec_subprocess();
    }
    else
    {
        return -1;
    }
}
