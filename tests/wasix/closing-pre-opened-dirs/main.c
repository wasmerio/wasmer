#include <stdio.h>
#include <stdlib.h>
#include <dirent.h>
#include <unistd.h>
#include <string.h>
#include <assert.h>

int main() {
    const char *expected_entries[] = {".", "..", "main.c", "main.wasm", "output", "run.sh", NULL};

    for (int fd = 0; fd <= 5; fd++) {
        close(fd);
    }

    DIR *dir = opendir(".");
    if (!dir) {
        printf("opendir");
        exit(EXIT_FAILURE);
    }

    struct dirent *entry;
    int i = 0;
    while ((entry = readdir(dir)) != NULL) {
        if (expected_entries[i] == NULL) {
            fprintf(stdout, "Unexpected extra entry: %s\n", entry->d_name);
            closedir(dir);
            exit(EXIT_FAILURE);
        }

        int cmp_result = strcmp(entry->d_name, expected_entries[i]);
        if (cmp_result != 0) {
            printf("1");
            return 1;
        }

        i++;
    }

    if (expected_entries[i] != NULL) {
      printf("1");
      return 1;
    }

    closedir(dir);

    printf("0");
    return 0;
}

