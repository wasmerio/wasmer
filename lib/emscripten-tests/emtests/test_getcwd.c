#include <stdio.h>
#include <unistd.h>

int main() {
    const unsigned int size = 256;
    char cwd[size] = {};
    char* buf = getcwd(cwd, size);
    printf("getcwd\n");
    return 0;
}
