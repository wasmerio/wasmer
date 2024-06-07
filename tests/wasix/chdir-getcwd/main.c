#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>

int main()
{
    char cwd[1024];

    int status = EXIT_FAILURE;

    if (chdir("/tmp") != 0)
    {
        goto end;
    }

    if (getcwd(cwd, sizeof(cwd)) == NULL)
    {
        goto end;
    }

    if (strcmp(cwd, "/tmp") == 0)
    {
        status = EXIT_SUCCESS;
    }

end:
    printf("%d", status);
    exit(status);
}
