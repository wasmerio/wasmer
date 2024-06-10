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

    int status = 1;
    pid_t pid = fork();
    if (pid == -1)
    {
        goto end;
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
        waitpid(pid, &status, 0);
    }

end:
    printf("%d", status);
}