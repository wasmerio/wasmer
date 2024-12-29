#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <sys/types.h>

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

void error(const char *message)
{
    perror(message);
    exit(-1);
}

int main()
{
    if (mkdir("test1", S_IRWXU | S_IRWXG | S_IRWXO) != 0 || ensure_dir_accessible("test1") != 0)
    {
        error("test1");
    }

    if (mkdir("./test2", S_IRWXU | S_IRWXG | S_IRWXO) != 0 || ensure_dir_accessible("test2") != 0)
    {
        error("test2");
    }

    int cwd_fd = open(".", O_RDONLY | O_DIRECTORY);
    if (cwd_fd < 0)
    {
        error("open cwd");
    }

    if (mkdirat(cwd_fd, "test3", S_IRWXU | S_IRWXG | S_IRWXO) != 0 || ensure_dir_accessible("test3") != 0)
    {
        error("test3");
    }

    if (mkdirat(cwd_fd, "./test4", S_IRWXU | S_IRWXG | S_IRWXO) != 0 || ensure_dir_accessible("test4") != 0)
    {
        error("test4");
    }

    printf("0");
    return 0;
}
