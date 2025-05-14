#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <sys/wait.h>
#include <sys/stat.h>
#include <spawn.h>
#include <signal.h>

int run_tests()
{
    posix_spawnattr_t attr;
    posix_spawn_file_actions_t fdops;
    sigset_t sigdefault;
    pid_t pid;
    char *argv[] = {"main.wasm", "subprocess", NULL};
    char *envp[] = {"ABCD=1234", NULL};

    // Ignore both SIGTERM and SIGHUP, then set SIGHUP up to be reset to default
    if (signal(SIGTERM, SIG_IGN) == SIG_ERR)
    {
        perror("signal");
        return 1;
    }
    if (signal(SIGHUP, SIG_IGN) == SIG_ERR)
    {
        perror("signal");
        return 1;
    }

    // Raise the signal once, just to make sure it's _actually_ being ignored
    raise(SIGTERM);

    if (posix_spawnattr_init(&attr) != 0)
    {
        perror("posix_spawnattr_init");
        return 1;
    }
    sigemptyset(&sigdefault);
    sigaddset(&sigdefault, SIGHUP);
    if (posix_spawnattr_setsigdefault(&attr, &sigdefault) != 0)
    {
        perror("posix_spawnattr_setsigdefault");
        return 1;
    }
    if (posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETSIGDEF) != 0)
    {
        perror("posix_spawnattr_setflags");
        return 1;
    }

    // Open zzz on 11
    int fd = open("./output.zzz", O_WRONLY | O_CREAT, 0);
    if (fd < 0)
    {
        perror("open");
        return 1;
    }
    if (dup2(fd, 11) != 11)
    {
        perror("dup2");
        return 1;
    }
    if (dup2(fd, 13) != 13)
    {
        perror("dup2");
        return 1;
    }
    if (fcntl(13, F_SETFD, FD_CLOEXEC) == -1)
    {
        perror("fcntl");
        return 1;
    }
    if (close(fd) != 0)
    {
        perror("close");
        return 1;
    }

    posix_spawn_file_actions_init(&fdops);
    // Open yyy on 10
    posix_spawn_file_actions_addopen(&fdops, 10, "./output.yyy", O_WRONLY | O_CREAT, 0);
    // Renumber zzz to 12
    posix_spawn_file_actions_adddup2(&fdops, 11, 12);
    // Close 11
    posix_spawn_file_actions_addclose(&fdops, 11);
    // Request for 3 to be closed, but we expect it to remain open since it's a pre-open
    posix_spawn_file_actions_addclose(&fdops, 3);
    // After all of this, the subprocess should have 10 and 12, but not 11

    if (posix_spawn(&pid, "./main-not-asyncified.wasm", &fdops, &attr, argv, envp) != 0)
    {
        perror("posix_spawn");
        return 1;
    }

    if (posix_spawn_file_actions_destroy(&fdops) != 0)
    {
        perror("posix_spawn_file_actions_destroy");
        return 1;
    }
    if (posix_spawnattr_destroy(&attr) != 0)
    {
        perror("posix_spawnattr_destroy");
        return 1;
    }

    int status;
    if (waitpid(pid, &status, 0) == -1)
    {
        perror("waitpid");
        return 1;
    }

    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0)
    {
        printf("Child process failed with: %d\n", WEXITSTATUS(status));
        return 1;
    }

    char buffer[5] = {0};
    fd = open("./output.yyy", O_RDONLY | O_CREAT, 0);
    if (fd < 0)
    {
        perror("open");
        return 1;
    }
    int r = read(fd, buffer, 5);
    if (r <= 0)
    {
        perror("read from yyy");
    }
    if (strcmp(buffer, "yyy"))
    {
        printf("Expected yyy, got: %s\n", buffer);
        return 1;
    }
    if (close(fd) != 0)
    {
        perror("close");
        return 1;
    }

    fd = open("./output.zzz", O_RDONLY | O_CREAT, 0);
    if (fd < 0)
    {
        perror("open");
        return 1;
    }
    r = read(fd, buffer, 5);
    if (r <= 0)
    {
        perror("read from yyy");
    }
    if (strcmp(buffer, "zzz"))
    {
        printf("Expected zzz, got: %s\n", buffer);
        return 1;
    }
    if (close(fd) != 0)
    {
        perror("close");
        return 1;
    }

    char *argv2[] = {"main.wasm", "just-return", NULL};
    putenv("PATH=/home/");
    status = 0;

    if (posix_spawnp(&pid, "main-not-asyncified.wasm", NULL, NULL, argv2, NULL) != 0)
    {
        perror("posix_spawn 2");
        return 1;
    }

    if (waitpid(pid, &status, 0) == -1)
    {
        perror("waitpid 2");
        return 1;
    }

    if (!WIFEXITED(status) || WEXITSTATUS(status) != 70)
    {
        printf("Expected exit with status 70, got: %d\n", WEXITSTATUS(status));
        return 1;
    }

    return 0;
}

// Since we don't pipe stderr from the child process, this function writes
// output to a file which can (hopefully!) be inspected
void write_subprocess_error(const char *msg)
{
    FILE *outf = fopen("./output.child", "w");
    if (!outf)
    {
        exit(EXIT_FAILURE);
    }
    fprintf(outf, "%s: %s\n", msg, strerror(errno));
    fclose(outf);
    exit(EXIT_FAILURE);
}

int subprocess(int argc, char **argv)
{
    if (argc != 2 || strcmp(argv[0], "main.wasm"))
    {
        write_subprocess_error("Got bad CLI args");
        return 2;
    }

    if (!strcmp(argv[1], "just-return"))
    {
        return 70;
    }
    else if (strcmp(argv[1], "subprocess"))
    {
        write_subprocess_error("Got bad CLI args");
        return 2;
    }

    const char *env = getenv("ABCD");
    if (strcmp(env, "1234"))
    {
        char buf[128];
        sprintf(buf, "env var not set correctly, value is: %s", env);
        write_subprocess_error(buf);
    }

    struct sigaction act = {0};
    if (sigaction(SIGHUP, NULL, &act) != 0)
    {
        write_subprocess_error("sigaction");
    }
    if (act.sa_handler != SIG_DFL)
    {
        write_subprocess_error("expected SIGHUP to be set to SIG_DFL");
    }

    if (sigaction(SIGTERM, NULL, &act) != 0)
    {
        write_subprocess_error("sigaction");
    }
    if (act.sa_handler != SIG_IGN)
    {
        write_subprocess_error("expected SIGTERM to be set to SIG_IGN");
    }
    // and raise it once, just in case!
    raise(SIGTERM);

    int flags = fcntl(11, F_GETFD);
    if (flags != -1 || errno != EBADF)
    {
        write_subprocess_error("Expected EBADF for fd 11");
    }
    errno = 0;

    // 13 should be closed due to FD_CLOEXEC
    flags = fcntl(13, F_GETFD);
    if (flags != -1 || errno != EBADF)
    {
        write_subprocess_error("Expected EBADF for fd 11");
    }
    errno = 0;

    if (write(10, "yyy", 3) <= 0)
    {
        write_subprocess_error("write to yyy failed");
    }
    if (close(10) < 0)
    {
        write_subprocess_error("close(10) failed");
    }

    if (write(12, "zzz", 3) <= 0)
    {
        write_subprocess_error("write to zzz failed");
    }
    if (close(12) < 0)
    {
        write_subprocess_error("close(12) failed");
    }

    // 3 is a pre-open, and should remain open
    struct stat st;
    if (fstat(3, &st) != 0)
    {
        write_subprocess_error("failed to fstat pre-opened FD 3");
    }

    return 0;
}

int main(int argc, char **argv)
{
    if (argc >= 2)
    {
        return subprocess(argc, argv);
    }

    return run_tests();
}
