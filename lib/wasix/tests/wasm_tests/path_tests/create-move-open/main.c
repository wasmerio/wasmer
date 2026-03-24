// There used to be an issue where, when moving a file, you could no longer
// open it afterwards. This is a regression test for that issue.

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

void error(const char *message)
{
    perror(message);
    exit(-1);
}

int main()
{
    // Create two directories
    if (mkdir("test1", S_IRWXU))
    {
        error("mkdir test1");
    }
    if (mkdir("test1/inner", S_IRWXU))
    {
        error("mkdir t2");
    }

    FILE *file;
    if (!(file = fopen("test1/inner/file", "wt")))
    {
        error("create file");
    }
    if (!fwrite("hello\n", 1, 6, file))
    {
        error("write to file");
    }
    if (fflush(file))
    {
        error("fflush");
    }
    if (fclose(file))
    {
        error("fclose");
    }

    if (rename("test1", "test2"))
    {
        error("rename");
    }

    if (!(file = fopen("test2/inner/file", "rt")))
    {
        error("open renamed file");
    }
    char buf[7] = {0};
    if (!fread((void *)buf, 1, 7, file))
    {
        error("fread");
    }
    if (strcmp(buf, "hello\n"))
    {
        printf("Invalid file contents: %s\n", buf);
        return -1;
    }

    printf("0");
    return 0;
}
