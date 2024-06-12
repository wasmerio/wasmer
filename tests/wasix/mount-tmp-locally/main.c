#include <stdio.h>
#include <stdlib.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <unistd.h>

int main() {
    int status = 1;

    if (mkdir("/tmp/my_test_dir", 0777) == -1) {
        goto end;
    }

    status = access("/tmp/my_test_dir", F_OK) != 0;

end:
    printf("%d", status);
}
