#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>

static void fail(const char *message)
{
    perror(message);
    exit(1);
}

int main(void)
{
    struct stat left;
    struct stat right;

    if (lstat("/a/link.txt", &left) != 0)
    {
        fail("lstat /a/link.txt");
    }
    if (lstat("/b/link.txt", &right) != 0)
    {
        fail("lstat /b/link.txt");
    }

    if (!S_ISLNK(left.st_mode) || !S_ISLNK(right.st_mode))
    {
        fprintf(stderr, "expected both paths to remain symlinks\n");
        return 1;
    }

    if (left.st_ino == right.st_ino)
    {
        fprintf(stderr,
                "expected distinct symlink inode ids, got left=%llu right=%llu\n",
                (unsigned long long)left.st_ino,
                (unsigned long long)right.st_ino);
        return 1;
    }

    printf("0");
    return 0;
}
