//#ExpectedStdout: 0

#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <string.h>
#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>

static void assert_dir_has_entry(const char *path, const char *expected)
{
    DIR *dir = opendir(path);
    assert(dir != NULL);

    int found = 0;
    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL)
    {
        if (strcmp(entry->d_name, expected) == 0)
        {
            found = 1;
            break;
        }
    }

    assert(closedir(dir) == 0);
    assert(found);
}

static void assert_dir_has_no_entry(const char *path, const char *forbidden)
{
    DIR *dir = opendir(path);
    assert(dir != NULL);

    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL)
    {
        assert(strcmp(entry->d_name, forbidden) != 0);
    }

    assert(closedir(dir) == 0);
}

int main(void)
{
    assert_dir_has_entry("./d1", "d2l");
    assert_dir_has_no_entry("./d1", "outside");

    errno = 0;
    int fd = open("./d1/d2l/test.txt", O_RDONLY);
    assert(fd >= 0);
    assert(close(fd) == 0);

    errno = 0;
    fd = open("./d1/outside/main.c", O_RDONLY);
    assert(fd < 0);

    printf("0");
    return 0;
}
