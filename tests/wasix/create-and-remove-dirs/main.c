#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

int ensure_dir_accessible(const char *dir_name)
{
    struct stat st;
    char buf[256];

    if (stat(dir_name, &st) != 0 || !S_ISDIR(st.st_mode))
    {
        return -1;
    }

    sprintf(buf, "./%s", dir_name);
    if (stat(buf, &st) != 0 || !S_ISDIR(st.st_mode))
    {
        return -1;
    }

    sprintf(buf, "/home/%s", dir_name);
    if (stat(buf, &st) != 0 || !S_ISDIR(st.st_mode))
    {
        return -1;
    }

    return 0;
}

int ensure_dir_removed(const char *dir_name)
{
    struct stat st;

    if (stat(dir_name, &st) == 0 || errno != ENOENT)
    {
        return -1;
    }

    errno = 0;
    return 0;
}

void error(const char *message)
{
    perror(message);
    exit(-1);
}

int main()
{
    if (mkdir("test1/test2", S_IRWXU | S_IRWXG | S_IRWXO) == 0)
    {
        printf("Expected recursive directory creation to fail\n");
        return -1;
    }

    if (mkdir("test1", S_IRWXU | S_IRWXG | S_IRWXO) != 0 || ensure_dir_accessible("test1") != 0)
    {
        error("mkdir test1");
    }

    if (mkdir("test1/test2", S_IRWXU | S_IRWXG | S_IRWXO) != 0 || ensure_dir_accessible("test1/test2") != 0)
    {
        error("mkdir test2");
    }

    if (rmdir("test1") == 0)
    {
        printf("Expected removing non-empty directory to fail\n");
        return -1;
    }

    if (rmdir("test1/test2") != 0 || ensure_dir_removed("test1/test2") != 0)
    {
        error("rmdir test2");
    }

    if (rmdir("test1") != 0 || ensure_dir_removed("test1") != 0)
    {
        error("rmdir test1");
    }

    if (mkdir("test1", S_IRWXU | S_IRWXG | S_IRWXO) != 0 || ensure_dir_accessible("test1") != 0)
    {
        error("re-create test1");
    }

    if (rmdir("test1") != 0 || ensure_dir_removed("test1") != 0)
    {
        error("re-remove test1");
    }

    printf("0");
    return 0;
}
