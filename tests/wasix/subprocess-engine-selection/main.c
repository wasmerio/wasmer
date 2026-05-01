#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

int main(void) {
    pid_t pid = vfork();
    if (pid < 0) {
        perror("vfork");
        return 1;
    }

    if (pid == 0) {
        execl("./child.wasm", "child.wasm", NULL);
        perror("execl");
        _exit(127);
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        perror("waitpid");
        return 1;
    }

    if (!WIFEXITED(status)) {
        fprintf(stderr, "child did not exit normally\n");
        return 1;
    }

    int exit_code = WEXITSTATUS(status);
    if (exit_code != 42) {
        fprintf(stderr, "expected exit code 42 from child, got %d\n", exit_code);
        return 1;
    }

    return 0;
}
