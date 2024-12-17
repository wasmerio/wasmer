#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
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
    if (mkdir("test2", S_IRWXU))
    {
        error("mkdir test2");
    }

    // open the second directory for fstatat
    int fd;
    if ((fd = open("test2", O_RDONLY | O_DIRECTORY)) < 0)
    {
        error("open");
    }

    // chdir into the first directory
    if (chdir("/home/test1"))
    {
        error("chdir");
    }

    // Now stat the second directory with a relative path.
    // CWD should not be taken into account, and the stat
    // should succeed.
    struct stat st;
    if (fstatat(fd, ".", &st, 0))
    {
        error("fstatat");
    }

    if (!S_ISDIR(st.st_mode))
    {
        printf("Expected a directory\n");
        return -1;
    }

    printf("0");
    return 0;
}
