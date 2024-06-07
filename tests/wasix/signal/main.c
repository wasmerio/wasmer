#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <signal.h>
#include <sys/wait.h>

void sig_handler(int signo)
{
    exit(signo != SIGHUP);
}

int main(int argc, char *argv[])
{
pid_t pid;
    int status;

    pid = fork();

    if (pid == -1) {
        return EXIT_FAILURE;
    } else if (pid == 0) {
        signal(SIGHUP, sig_handler);
        while (1) {
            sleep(1);
        }
    } else {
        sleep(1);

        kill(pid, SIGHUP);

        waitpid(pid, &status, 0);
        
        printf("%d", status);
    }
}