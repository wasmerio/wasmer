#include <stdio.h>
#include <unistd.h>

int main() {
    char command[] = "C:\\Windows\\System32\\cmd.exe";
    char arg1[] = "echo";
    char arg2[] = "foo";
    char* argv[3];
    argv[0] = arg1;
    argv[1] = arg2;
    argv[2] = 0;
    printf("_execvp\n");
    int result = execvp(command, argv);
    // should not return, and not print this message
    printf("error");
    return 0;
}
