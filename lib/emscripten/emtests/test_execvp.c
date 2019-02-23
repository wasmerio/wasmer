#include <stdio.h>
#include <unistd.h>

int main() {
    char command[] = "echo";
    char arg1[] = "foo";
    char arg2[] = "bar";
    char* argv[] = {arg1, arg2};

    execvp(command, argv);

    printf("_execvp\n");

    return 0;
}
