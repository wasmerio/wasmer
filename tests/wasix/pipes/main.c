#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
#include <fcntl.h>
#include <errno.h>
#include <sys/wait.h>
#include <signal.h>
// Needed for PIPE_BUF
// TODO: wasi sysroot does not expose PIPE_BUF, but it should
// once it does, the hardcoded value should be removed.
#include <limits.h>
 #ifndef PIPE_BUF
 #define PIPE_BUF 4096
 #endif

int read_write()
{
    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return 1;
    }

    if (write(pipefd[1], "hello", 5) <= 0)
    {
        perror("write");
        return 1;
    }

    char buf[6];
    int r = read(pipefd[0], buf, 5);
    if (r <= 0)
    {
        perror("read");
        return 1;
    }

    buf[r] = '\0';

    if (strcmp(buf, "hello"))
    {
        printf("Got bad message from pipe: %s\n", buf);
        return 1;
    }

    return 0;
}

int read_from_closed_pipe()
{
    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return 1;
    }

    if (close(pipefd[1]) != 0)
    {
        perror("close");
        return 1;
    }

    char buf[1];
    int r = read(pipefd[0], buf, 1);
    // Should get EOF when reading from closed pipe
    if (r != 0)
    {
        perror("read");
        return 1;
    }

    return 0;
}

int sigpipe_witnessed = 0;

void handle_sigpipe(int sig)
{
    sigpipe_witnessed = 1;
}

int write_to_closed_pipe()
{
    signal(SIGPIPE, handle_sigpipe);

    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return 1;
    }

    if (close(pipefd[0]) != 0)
    {
        perror("close");
        return 1;
    }

    sigpipe_witnessed = 0;

    int r = write(pipefd[1], "hello", 5);
    if (r != -1 || errno != EPIPE)
    {
        printf("Expected write to fail with EPIPE, but got %d\n", errno);
        return 1;
    }

    if (!sigpipe_witnessed)
    {
        printf("Expected to catch SIGPIPE signal\n");
        return 1;
    }

    signal(SIGPIPE, SIG_DFL);
    return 0;
}

int multiple_readers()
{
    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return 1;
    }

    int read_dup = dup(pipefd[0]);
    if (read_dup < 0)
    {
        perror("dup");
        return 1;
    }

    if (write(pipefd[1], "hello and bye", 13) <= 0)
    {
        perror("write");
        return 1;
    }

    char buf[8];
    int r = read(pipefd[0], buf, 6);
    if (r <= 0)
    {
        perror("read");
        return 1;
    }

    buf[r] = '\0';

    if (strcmp(buf, "hello "))
    {
        printf("Got bad message from pipe: %s\n", buf);
        return 1;
    }

    r = read(read_dup, buf, 7);
    if (r <= 0)
    {
        perror("read");
        return 1;
    }

    buf[r] = '\0';

    if (strcmp(buf, "and bye"))
    {
        printf("Got bad message from pipe: %s\n", buf);
        return 1;
    }

    return 0;
}

int multiple_writers()
{
    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return 1;
    }

    int write_dup = dup(pipefd[1]);
    if (write_dup < 0)
    {
        perror("dup");
        return 1;
    }

    if (write(pipefd[1], "hello ", 6) <= 0)
    {
        perror("write");
        return 1;
    }

    // Need to close the write ends, otherwise the read loop below hangs forever
    if (close(pipefd[1]) != 0)
    {
        perror("close");
        return 1;
    }

    if (write(write_dup, "and bye", 7) <= 0)
    {
        perror("write");
        return 1;
    }

    if (close(write_dup) != 0)
    {
        perror("close");
        return 1;
    }

    char buf[14];
    char *ptr = buf;
    int r;
    for (;;)
    {
        r = read(pipefd[0], ptr, 13 - (ptr - buf));
        if (r < 0)
        {
            perror("read");
            return 1;
        }
        else if (r == 0)
        {
            break;
        }
        ptr += r;
    }

    *ptr = 0;

    if (strcmp(buf, "hello and bye"))
    {
        printf("Got bad message from pipe: %s\n", buf);
        return 1;
    }

    return 0;
}

int fork_subprocess(int fd)
{
    char buf[6];
    int r = read(fd, buf, 5);
    if (r <= 0)
    {
        perror("read");
        return 1;
    }

    buf[r] = '\0';

    if (strcmp(buf, "hello"))
    {
        printf("Got bad message from pipe: %s\n", buf);
        return 1;
    }

    r = read(fd, buf, 5);
    if (r != 0)
    {
        perror("read");
        return 1;
    }

    return 0;
}

int across_fork()
{
    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return 1;
    }

    // Unless the write end is closed in the subprocess as well, the read will hang forever
    if (fcntl(pipefd[1], F_SETFD, FD_CLOEXEC) != 0)
    {
        perror("fcntl");
        return 1;
    }

    pid_t pid = fork();
    if (pid < 0)
    {
        perror("fork");
        return 1;
    }

    if (pid == 0)
    {
        char buf[5];
        sprintf(buf, "%d", pipefd[0]);
        execle("./main.wasm", "main.wasm", "fork_subprocess", buf, NULL, NULL);
        perror("execle");
        return 1;
    }

    if (close(pipefd[0]) != 0)
    {
        perror("close pipefd[0]");
        return 1;
    }

    if (write(pipefd[1], "hello", 5) <= 0)
    {
        perror("write");
        return 1;
    }

    if (close(pipefd[1]) != 0)
    {
        perror("close pipefd[0]");
        return 1;
    }

    int status;
    if (waitpid(pid, &status, 0) != pid)
    {
        perror("waitpid");
        return 1;
    }

    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0)
    {
        printf("Subprocess failed\n");
        return 1;
    }

    return 0;
}

int vfork_subprocess(int fd)
{
    char buf[6];
    int r = read(fd, buf, 5);
    if (r <= 0)
    {
        perror("read");
        return 1;
    }

    buf[r] = '\0';

    if (strcmp(buf, "hello"))
    {
        printf("Got bad message from pipe: %s\n", buf);
        return 1;
    }

    r = read(fd, buf, 5);
    if (r != 0)
    {
        perror("read");
        return 1;
    }

    return 0;
}

int across_vfork()
{
    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return 1;
    }

    // Unless the write end is closed in the subprocess as well, the read will hang forever
    if (fcntl(pipefd[1], F_SETFD, FD_CLOEXEC) != 0)
    {
        perror("fcntl");
        return 1;
    }

    pid_t pid = vfork();
    if (pid < 0)
    {
        perror("vfork");
        return 1;
    }

    if (pid == 0)
    {
        char buf[5];
        sprintf(buf, "%d", pipefd[0]);
        execle("./main.wasm", "main.wasm", "vfork_subprocess", buf, NULL, NULL);
        perror("execle");
        return 1;
    }

    if (close(pipefd[0]) != 0)
    {
        perror("close pipefd[0]");
        return 1;
    }

    if (write(pipefd[1], "hello", 5) <= 0)
    {
        perror("write");
        return 1;
    }

    if (close(pipefd[1]) != 0)
    {
        perror("close pipefd[0]");
        return 1;
    }

    int status;
    if (waitpid(pid, &status, 0) != pid)
    {
        perror("waitpid");
        return 1;
    }

    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0)
    {
        printf("Subprocess failed\n");
        return 1;
    }

    return 0;
}


int nonblocking_eagain()
{
    int pipefd[2];
    if (pipe(pipefd) != 0)
    {
        perror("pipe");
        return 1;
    }

    // Set write end to non-blocking
    int flags = fcntl(pipefd[1], F_GETFL, 0);
    if (flags < 0)
    {
        perror("fcntl F_GETFL");
        return 1;
    }
    if (fcntl(pipefd[1], F_SETFL, flags | O_NONBLOCK) != 0)
    {
        perror("fcntl F_SETFL");
        return 1;
    }

    // Write until the buffer is full
    int bytes_written = 0;
    while (1)
    {
        int r = write(pipefd[1], "x", 1);
        if (r == 1)
        {
            bytes_written++;
            // Safety check — WASIX bug: buffer is unbounded and never fills up
            if (bytes_written > 1024 * 1024) // 1MB, way above any sane pipe buffer
            {
                printf("BUG: wrote %d bytes without EAGAIN — pipe buffer is unbounded\n",
                       bytes_written);
                return 1;
            }
        }
        else if (r == -1 && errno == EAGAIN)
        {
            // Correct behavior — buffer is full
            break;
        }
        else
        {
            perror("write");
            return 1;
        }
    }

    // Drain some data
    char buf[1024];
    int r = read(pipefd[0], buf, sizeof(buf));
    if (r <= 0)
    {
        perror("read");
        return 1;
    }

    // After draining, we should be able to write again
    r = write(pipefd[1], "y", 1);
    if (r != 1)
    {
        printf("Expected write to succeed after draining, but got %d (errno=%d)\n", r, errno);
        return 1;
    }

    printf("OK: pipe buffer filled after %d bytes, EAGAIN returned correctly\n", bytes_written);
    return 0;
}

// Test that pipe capacity is bounded at a sane value (not just 1MB)
int pipe_buf_bounded()
{
    int pipefd[2];
    if (pipe(pipefd) != 0) { perror("pipe"); return 1; }

    // Set non-blocking
    int flags = fcntl(pipefd[1], F_GETFL, 0);
    fcntl(pipefd[1], F_SETFL, flags | O_NONBLOCK);

    int bytes_written = 0;
    while (1)
    {
        int r = write(pipefd[1], "x", 1);
        if (r == 1) {
            bytes_written++;
            if (bytes_written > 2 * 1024 * 1024) {
                printf("BUG: pipe buffer exceeds 2MB (%d bytes)\n", bytes_written);
                return 1;
            }
        }
        else if (r == -1 && errno == EAGAIN) break;
        else { perror("write"); return 1; }
    }

    // POSIX requires at least 512 bytes, Linux uses 65536
    if (bytes_written < 512) {
        printf("BUG: pipe buffer too small (%d bytes)\n", bytes_written);
        return 1;
    }

    printf("OK: pipe buffer capacity = %d bytes\n", bytes_written);
    close(pipefd[0]);
    close(pipefd[1]);
    return 0;
}

// Test that read on empty non-blocking pipe returns EAGAIN
int nonblocking_read_eagain()
{
    int pipefd[2];
    if (pipe(pipefd) != 0) { perror("pipe"); return 1; }

    int flags = fcntl(pipefd[0], F_GETFL, 0);
    fcntl(pipefd[0], F_SETFL, flags | O_NONBLOCK);

    char buf[1];
    int r = read(pipefd[0], buf, 1);
    if (r != -1 || errno != EAGAIN)
    {
        printf("Expected EAGAIN on empty non-blocking read, got r=%d errno=%d\n", r, errno);
        return 1;
    }

    close(pipefd[0]);
    close(pipefd[1]);
    printf("OK: empty non-blocking read returns EAGAIN\n");
    return 0;
}

// Test O_NONBLOCK write resumes correctly after drain
int nonblocking_write_resumes_after_drain()
{
    int pipefd[2];
    if (pipe(pipefd) != 0) { perror("pipe"); return 1; }

    int flags = fcntl(pipefd[1], F_GETFL, 0);
    fcntl(pipefd[1], F_SETFL, flags | O_NONBLOCK);

    // Fill pipe
    while (write(pipefd[1], "x", 1) == 1) {}

    // Drain 1KB
    char drain[1024];
    int drained = read(pipefd[0], drain, sizeof(drain));
    if (drained <= 0) { perror("drain read"); return 1; }

    // Should be able to write again
    int r = write(pipefd[1], "y", 1);
    if (r != 1)
    {
        printf("Expected write to succeed after drain, got %d errno=%d\n", r, errno);
        return 1;
    }

    close(pipefd[0]);
    close(pipefd[1]);
    printf("OK: non-blocking write resumes after drain\n");
    return 0;
}

// Test that data survives close of write end (EOF only after drain)
int data_survives_write_close()
{
    int pipefd[2];
    if (pipe(pipefd) != 0) { perror("pipe"); return 1; }

    if (write(pipefd[1], "hello", 5) != 5) { perror("write"); return 1; }
    close(pipefd[1]);

    // Data should still be readable
    char buf[6];
    int r = read(pipefd[0], buf, 5);
    if (r != 5) { printf("Expected 5 bytes, got %d\n", r); return 1; }
    buf[r] = '\0';
    if (strcmp(buf, "hello")) { printf("Got bad data: %s\n", buf); return 1; }

    // Now EOF
    r = read(pipefd[0], buf, 1);
    if (r != 0) { printf("Expected EOF, got %d\n", r); return 1; }

    close(pipefd[0]);
    printf("OK: data survives write end close\n");
    return 0;
}

// Test dup2 redirects correctly through a pipe
int pipe_dup2_redirect()
{
    int pipefd[2];
    if (pipe(pipefd) != 0) { perror("pipe"); return 1; }

    // Redirect stdout to pipe write end
    int saved_stdout = dup(STDOUT_FILENO);
    dup2(pipefd[1], STDOUT_FILENO);
    close(pipefd[1]);

    printf("piped");
    fflush(stdout);

    // Restore stdout
    dup2(saved_stdout, STDOUT_FILENO);
    close(saved_stdout);

    char buf[7];
    ssize_t total = 0;
    while (total < 6)
    {
     ssize_t r = read(pipefd[0], buf + total, 6 - total);
     if (r < 0)
     {
         perror("read");
         close(pipefd[0]);
         return 1;
     }
     if (r == 0)
     {
         break;
     }
     total += r;
    }
    buf[total] = '\0';
    close(pipefd[0]);

    if (strcmp(buf, "piped"))
    {
        printf("Expected 'piped', got '%s'\n", buf);
        return 1;
    }

    printf("OK: dup2 redirect through pipe works\n");
    return 0;
}

// Test PIPE_BUF atomicity — write of <= PIPE_BUF must not be partial
int atomic_write_pipe_buf()
{
    int pipefd[2];
    if (pipe(pipefd) != 0) { perror("pipe"); return 1; }

    int flags = fcntl(pipefd[1], F_GETFL, 0);
    fcntl(pipefd[1], F_SETFL, flags | O_NONBLOCK);

    // Write exactly PIPE_BUF bytes — must be atomic (all or nothing)
    char data[PIPE_BUF];
    memset(data, 0xAB, PIPE_BUF);

    int r = write(pipefd[1], data, PIPE_BUF);
    if (r != -1 && r != PIPE_BUF)
    {
        printf("BUG: atomic write of PIPE_BUF returned partial %d bytes\n", r);
        return 1;
    }

    if (r == PIPE_BUF)
        printf("OK: PIPE_BUF atomic write succeeded (%d bytes)\n", r);
    else
    {
        if (errno != EAGAIN)
        {
            printf("BUG: atomic write of PIPE_BUF failed with errno %d (%s), expected EAGAIN\n",
                errno,
                strerror(errno));
            close(pipefd[0]);
            close(pipefd[1]);
            return 1;
        }
        printf("OK: PIPE_BUF atomic write correctly returned EAGAIN when full\n");
    }

    close(pipefd[0]);
    close(pipefd[1]);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc >= 3 && !strcmp(argv[1], "fork_subprocess")) { return fork_subprocess(atoi(argv[2])); }
    if (argc >= 3 && !strcmp(argv[1], "vfork_subprocess")) { return vfork_subprocess(atoi(argv[2])); }

    if (read_write() != 0) { return 1;  }
    if (read_from_closed_pipe() != 0) { return 1;  }

    if (write_to_closed_pipe() != 0) { return 1;  }

    if (multiple_readers() != 0) { return 1;  }
    if (multiple_writers() != 0) { return 1;  }

    if (across_fork() != 0) { return 1;  }
    if (across_vfork() != 0) { return 1; }

    if (nonblocking_eagain() != 0) { return 1; }
    if (pipe_buf_bounded() != 0) return 1;

    if (nonblocking_read_eagain() != 0) return 1;
    if (nonblocking_write_resumes_after_drain() != 0) return 1;

    if (data_survives_write_close() != 0) return 1;
    if (pipe_dup2_redirect() != 0) return 1;
    if (atomic_write_pipe_buf() != 0) return 1;

    return 0;
}
