#include <stdio.h>
#include <dirent.h>
#include <unistd.h>
#include <string.h>

int main(int argc, char *argv[])
{
    DIR *dir;
    struct dirent *entry;

    dir = opendir("./");
    if (dir == NULL)
    {
        perror("opendir");
        return 1;
    }

    while ((entry = readdir(dir)) != NULL)
    {
        printf("%s\n", entry->d_name);
    }

    closedir(dir);

    return 0;
}
