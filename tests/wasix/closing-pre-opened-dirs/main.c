#include <stdio.h>
#include <stdlib.h>
#include <dirent.h>
#include <unistd.h>
#include <string.h>
#include <assert.h>

typedef unsigned char bool;
#define false (0)
#define true (1)

int main()
{
    const size_t expected_count = 7;
    const char *expected_entries[] = {".", "..", "main.c", "main.wasm", "main-not-asyncified.wasm", "output", "run.sh"};
    bool entries_observed[7] = {false};

    for (int fd = 3; fd <= 5; fd++)
    {
        close(fd);
    }

    DIR *dir = opendir(".");
    if (!dir)
    {
        printf("opendir");
        exit(EXIT_FAILURE);
    }

    struct dirent *entry;
    size_t total_count = 0;
    while ((entry = readdir(dir)) != NULL)
    {
        bool found = false;

        for (int i = 0; i < expected_count; ++i)
        {
            if (strcmp(expected_entries[i], entry->d_name) == 0 && entries_observed[i] == false)
            {
                entries_observed[i] = true;
                found = true;
                break;
            }
        }

        if (!found)
        {
            printf("Expected file name: %s\n", entry->d_name);
        }

        total_count++;
    }

    for (int i = 0; i < expected_count; ++i)
    {
        if (!entries_observed[i])
        {
            printf("Unobserved entry: %s\n", expected_entries[i]);
            exit(EXIT_FAILURE);
        }
    }

    if (total_count != expected_count)
    {
        printf("Mismatch in number of entries\n");
        exit(EXIT_FAILURE);
    }

    closedir(dir);

    printf("0");

    // Use fclose instead of close to make sure the write above is flushed
    fclose(stdin);
    fclose(stdout);
    fclose(stderr);

    // If this prints, it'll be caught in the output diff and fail the test.
    printf("Expected stdout to be closed\n");

    return 0;
}
