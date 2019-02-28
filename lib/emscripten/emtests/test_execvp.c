#include <stdio.h>
#include <unistd.h>

int main() {
    char command[] = "touch";
    char arg1[] = "foo.txt";
    char* argv[3];
    argv[0] = command;
    argv[1] = arg1;
    argv[2] = 0;

    printf("_execvp\n");
    int result = execvp(command, argv);
    // should not return, and not print this message
    printf("error");
    return 0;
}
