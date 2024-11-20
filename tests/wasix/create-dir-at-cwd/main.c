#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <sys/types.h>

int main() {
    int status = EXIT_FAILURE;

    const char *dirName1 = "test1";
    if (mkdir(dirName1, 0755) != 0) {
        goto end;
    }

    const char *dirName2 = "./test2";
    if (mkdir(dirName2, 0755) != 0) {
        goto end;
    }

    status = EXIT_SUCCESS;

end:
    printf("%d", status);
    return 0;
}
