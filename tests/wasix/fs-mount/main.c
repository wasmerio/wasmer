#include <stdio.h>
#include <string.h>

int main()
{
    FILE *f = fopen("/mount/testfile.txt", "r");
    if (f == NULL)
    {
        perror("Failed to open file");
        return 1;
    }

    char buffer[32];
    if (fgets(buffer, sizeof(buffer), f) != NULL)
    {
        if (strcmp(buffer, "Hello, Wasix!\n") != 0)
        {
            printf("Unexpected file content: %s", buffer);
            return 1;
        }
    }
    else
    {
        perror("Failed to read from file");
        return 1;
    }

    fclose(f);
    return 0;
}