#include <sys/types.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>

int main() {
    int mypipe[2];
    printf("pipe\n");
    if (pipe(mypipe)) {
        printf("error\n");
    }
    return 0;
}
