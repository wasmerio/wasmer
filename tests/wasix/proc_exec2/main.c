#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/wait.h>

int main(int argc, char *argv[])
{
    if (argc > 1 && argv[1] != NULL)
    {
        char *bar = getenv("foo");

        return (bar == NULL);
    }

    pid_t pid = fork();
    if (pid == -1)
    {
        exit(EXIT_FAILURE);
    }
    else if (pid == 0)
    {
        char *newargv[] = {argv[0], "child", NULL};
        char *newenviron[] = {"foo=bar", NULL};

        execve("/code/main.wasm", newargv, newenviron);

        exit(EXIT_FAILURE);
    }
    else
    {
        int status;
        waitpid(pid, &status, 0);
        printf("%d", status);
    }

    return 0;
}