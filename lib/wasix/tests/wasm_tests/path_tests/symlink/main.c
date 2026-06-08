//#ExpectedStdout: 0

#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <string.h>
#include <stdio.h>
#include <fcntl.h>

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
    assert_dir_has_no_entry("./d1", "d2l");

    errno = 0;
    assert(open("./d1/d2l/test.txt", O_RDONLY) < 0);
    assert(errno == ENOENT);

    printf("0");
    return 0;
}
