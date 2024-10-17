#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/wait.h>
#include <sys/types.h>
#include <sys/stat.h>

int main(int argc, char *argv[])
{
    if (argc > 1 && argv[1] != NULL)
    {
        if (mkdir("/tmp/child_test_dir", 0777) == -1)
        {
            exit(EXIT_FAILURE);
        }

        return access("/tmp/parent_test_dir", F_OK) != 0;
    }

    int status = 1;

    if (mkdir("/tmp/parent_test_dir", 0777) == -1)
    {
        goto end;
    }

    pid_t pid = fork();
    if (pid == -1)
    {
        goto end;
    }
    else if (pid == 0)
    {
        char *newargv[] = {argv[0], "child", NULL};

        execv("/code/main.wasm", newargv);

        exit(EXIT_FAILURE);
    }
    else
    {
        waitpid(pid, &status, 0);

        status = status | (access("/tmp/child_test_dir", F_OK) != 0);
    }

end:
    printf("%d", status);
}