#include <stdio.h>
#include <stdlib.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <unistd.h>
#include <sys/wait.h>

int main()
{
    pid_t pid;
    int status = 1;

    if (mkdir("/tmp/parent_test_dir", 0777) == -1)
    {
        goto end;
    }

    pid = fork();

    if (pid == -1)
    {
        goto end;
    }
    else if (pid == 0)
    {
        if (mkdir("/tmp/child_test_dir", 0777) == -1)
        {
            exit(EXIT_FAILURE);
        }

        return access("/tmp/parent_test_dir", F_OK) != 0;
    }
    else
    {
        waitpid(pid, &status, 0);

        status = status | (access("/tmp/child_test_dir", F_OK) != 0);
    }

end:
    printf("%d", status);
}
