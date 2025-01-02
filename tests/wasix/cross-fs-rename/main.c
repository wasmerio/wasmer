#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>

void error_exit(const char *desc, const char *call)
{
    const char *err_desc = strerror(errno);
    printf("Test \"%s\" failed at %s: %s\n", desc, call, err_desc);
    // Make it visible in the output log as well
    fprintf(stderr, "Test \"%s\" failed at %s: %s\n", desc, call, err_desc);
    exit(-1);
}

void create_and_move_file(const char *from_path, const char *to_path)
{
    char test_description[1024] = {0};
    sprintf(test_description, "%s -> %s", from_path, to_path);

    FILE *f = fopen(from_path, "wb");
    if (!f)
    {
        error_exit(test_description, "fopen");
    }

    char *txt = "hello";
    if (!fwrite(txt, 1, 6, f))
    {
        error_exit(test_description, "fwrite");
    }

    if (fclose(f))
    {
        error_exit(test_description, "fclose");
    }

    // /home is a host FS mount
    if (rename(from_path, to_path))
    {
        error_exit(test_description, "rename");
    }

    f = fopen(to_path, "rb");
    if (!f)
    {
        error_exit(test_description, "fopen 2");
    }

    char buffer[7] = {0};
    if (!fread(buffer, 1, 7, f))
    {
        error_exit(test_description, "fread");
    }

    if (strcmp(buffer, txt))
    {
        fprintf(stderr, "Expected %s to be equal to %s", buffer, txt);
        exit(-1);
    }

    if (fclose(f))
    {
        error_exit(test_description, "fclose 2");
    }
}

int main()
{
    // /tmp is on the MemFS, /temp1 and /temp2 are on separate HostFS instances

    // Move file within MemFS
    create_and_move_file("/tmp/old", "/tmp/new");

    // Move file within single mounted FS
    create_and_move_file("/temp1/old", "/temp1/new");

    // Move file from MemFS to mounted FS
    create_and_move_file("/tmp/file", "/temp1/file");

    // Move file from mounted FS to MemFS
    create_and_move_file("/temp1/file2", "/tmp/file2");

    // Move file between different mounted FS's
    create_and_move_file("/temp1/file3", "/temp2/file3");

    printf("0");
    return 0;
}
