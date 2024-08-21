#include <stdio.h>
#include <stdlib.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>
#include <stdio.h>
#include <dirent.h>

int main()
{
    DIR *dir;
    struct dirent *entry;

    printf("/:\n");
    dir = opendir("/");

    if (dir != NULL)
    {
        while ((entry = readdir(dir)) != NULL)
        {
            printf("%s\n", entry->d_name);
        }
        closedir(dir);
    }
    else
    {
        perror("opendir");
        return 1;
    }

    printf("\n/app:\n");
    dir = opendir("/app");

    if (dir != NULL)
    {
        while ((entry = readdir(dir)) != NULL)
        {
            printf("%s\n", entry->d_name);
        }
        closedir(dir);
    }
    else
    {
        perror("opendir");
        return 1;
    }

    printf("\n/app/a:\n");
    dir = opendir("/app/a");

    if (dir != NULL)
    {
        while ((entry = readdir(dir)) != NULL)
        {
            printf("%s\n", entry->d_name);
        }
        closedir(dir);
    }
    else
    {
        perror("opendir");
        return 1;
    }

    printf("\n/app/b:\n");
    dir = opendir("/app/b");

    if (dir != NULL)
    {
        while ((entry = readdir(dir)) != NULL)
        {
            printf("%s\n", entry->d_name);
        }
        closedir(dir);
    }
    else
    {
        perror("opendir");
        return 1;
    }

    return 0;
}