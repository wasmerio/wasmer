// Combined test file for popen stdin close issue
// Contains: echo, mysh (shell), vfork-based test, and popen-based test
// Dispatched by first command-line argument

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <spawn.h>
#include <sys/wait.h>
#include <wasi/libc-environ.h>
#define __environ __wasilibc_environ
extern char **__environ;
extern char **environ;

// ============================================================================
// mypopen/mypclose implementation - uses posix_spawn with our shell
// ============================================================================

// For the tests we store the pid in a global instead of the file as wasix-libc does not expose FILE internals.
// This does not change the behaviour as long as there is only a single popen at a time, which is the case for our tests.
static pid_t g_popen_pid = -1;

// Exact copy of wasix-libc's popen implementation, except using the global pid instead of f->pipe_pid
// Also no calls to FLOCK and FUNLOCK, but those are no-ops in wasix-libc anyway so that doesn't change behavior
FILE *mypopen(const char *cmd, const char *mode)
{
	int p[2], op, e;
	pid_t pid;
	FILE *f;
	posix_spawn_file_actions_t fa;

	__wasilibc_ensure_environ();

	if (*mode == 'r') {
		op = 0;
	} else if (*mode == 'w') {
		op = 1;
	} else {
		errno = EINVAL;
		return 0;
	}

	if (pipe2(p, O_CLOEXEC)) return NULL;
	f = fdopen(p[op], mode);
	if (!f) {
		close(p[0]);
		close(p[1]);
		return NULL;
	}
	// FLOCK(f);

	/* If the child's end of the pipe happens to already be on the final
	 * fd number to which it will be assigned (either 0 or 1), it must
	 * be moved to a different fd. Otherwise, there is no safe way to
	 * remove the close-on-exec flag in the child without also creating
	 * a file descriptor leak race condition in the parent. */
	if (p[1-op] == 1-op) {
		int tmp = fcntl(1-op, F_DUPFD_CLOEXEC, 0);
		if (tmp < 0) {
			e = errno;
			goto fail;
		}
		close(p[1-op]);
		p[1-op] = tmp;
	}

	e = ENOMEM;
	if (!posix_spawn_file_actions_init(&fa)) {
		if (!posix_spawn_file_actions_adddup2(&fa, p[1-op], 1-op)) {
			// Use /src/mysh.wasm instead of /bin/sh
			if (!(e = posix_spawn(&pid, "/src/main.wasm", &fa, 0,
			    (char *[]){ "/src/main.wasm", "shell", "-c", (char *)cmd, 0 }, environ))) {
				posix_spawn_file_actions_destroy(&fa);
				// f->pipe_pid = pid;
                g_popen_pid = pid;
				if (!strchr(mode, 'e'))
					fcntl(p[op], F_SETFD, 0);
				close(p[1-op]);
				// FUNLOCK(f);
				return f;
			}
		}
		posix_spawn_file_actions_destroy(&fa);
	}
fail:
	fclose(f);
	close(p[1-op]);
	errno = e;
	return 0;
}

// Exact copy of wasix-libc's pclose implementation, except using the global pid instead of f->pipe_pid
int mypclose(FILE *f)
{
	int status, r;
	// pid_t pid = f->pipe_pid;
    pid_t pid = g_popen_pid;
    g_popen_pid = -1;
	fclose(f);
	while ((r = waitpid(pid, &status, 0)) < 0 && errno == EINTR);
	if (r < 0) return errno;
	return status;
}


// ============================================================================
// Echo functionality - reads stdin until EOF, then writes to stdout
// ============================================================================

#define INITIAL_CAPACITY 4096

int do_echo(void)
{
    size_t capacity = INITIAL_CAPACITY;
    size_t total_size = 0;
    char *buffer = malloc(capacity);

    if (!buffer) {
        fprintf(stderr, "Memory allocation failed\n");
        return 1;
    }

    // Read from stdin until EOF, growing buffer as needed
    size_t bytes_read;
    char temp[4096];
    while ((bytes_read = fread(temp, 1, sizeof(temp), stdin)) > 0) {
        // Grow buffer if needed
        if (total_size + bytes_read > capacity) {
            capacity *= 2;
            char *new_buffer = realloc(buffer, capacity);
            if (!new_buffer) {
                fprintf(stderr, "Memory reallocation failed\n");
                free(buffer);
                return 1;
            }
            buffer = new_buffer;
        }

        // Copy data to buffer
        memcpy(buffer + total_size, temp, bytes_read);
        total_size += bytes_read;
    }

    // Now print the entire buffer to stdout
    fwrite(buffer, 1, total_size, stdout);
    fflush(stdout);

    free(buffer);
    return 0;
}

// ============================================================================
// Shell functionality - minimal shell that only supports: sh -c "<command>"
// ============================================================================

#define MAX_ARGS 64

// Simple tokenizer that splits command by spaces, respecting quotes
static int tokenize(char *cmd, char **argv, int max_args)
{
    int argc = 0;
    char *p = cmd;

    while (*p && argc < max_args - 1) {
        // Skip leading whitespace
        while (*p && isspace((unsigned char)*p)) {
            p++;
        }
        if (!*p) {
            break;
        }

        char *start;
        if (*p == '"' || *p == '\'') {
            // Quoted string
            char quote = *p++;
            start = p;
            while (*p && *p != quote) {
                p++;
            }
            if (*p == quote) {
                *p++ = '\0';
            }
        } else {
            // Unquoted token
            start = p;
            while (*p && !isspace((unsigned char)*p)) {
                p++;
            }
            if (*p) {
                *p++ = '\0';
            }
        }

        argv[argc++] = start;
    }

    argv[argc] = NULL;
    return argc;
}

int do_shell(int argc, char *argv[])
{
    // We expect: main.wasm shell -c "<command>"
    // argv[0] = "main.wasm", argv[1] = "shell", argv[2] = "-c", argv[3] = "<command>"
    if (argc < 4) {
        fprintf(stderr, "Usage: main.wasm shell -c \"command\"\n");
        return 1;
    }

    if (strcmp(argv[2], "-c") != 0) {
        fprintf(stderr, "shell: only -c option is supported\n");
        return 1;
    }

    // Copy the command so we can modify it
    char *cmd = strdup(argv[3]);
    if (!cmd) {
        perror("strdup");
        return 1;
    }

    // Tokenize the command
    char *exec_argv[MAX_ARGS];
    int exec_argc = tokenize(cmd, exec_argv, MAX_ARGS);

    if (exec_argc == 0) {
        fprintf(stderr, "shell: empty command\n");
        free(cmd);
        return 1;
    }

    // Execute the command
    execv(exec_argv[0], exec_argv);

    // If we get here, execv failed
    fprintf(stderr, "shell: execv failed for '%s': %s\n", exec_argv[0], strerror(errno));
    free(cmd);
    return 127;
}

// ============================================================================
// Vfork test - spawns echo directly using vfork (this works)
// ============================================================================

int do_vfork_test(void)
{
    int pipe_fd[2];
    pid_t pid;

    // Create a pipe
    if (pipe(pipe_fd) == -1) {
        perror("pipe failed");
        return 1;
    }

    // Fork a child process
    pid = vfork();
    if (pid == -1) {
        perror("vfork failed");
        return 1;
    }

    if (pid == 0) {
        // Child process: execute echo
        close(pipe_fd[1]); // Close write end

        // Redirect stdin to read end of pipe
        if (dup2(pipe_fd[0], STDIN_FILENO) == -1) {
            perror("dup2 failed");
            _exit(1);
        }
        close(pipe_fd[0]);

        // Execute echo directly without shell
        char *args[] = {"./main.wasm", "echo", NULL};
        execv("./main.wasm", args);

        // If execv returns, it failed
        perror("execv failed");
        _exit(1);
    } else {
        // Parent process: write to pipe
        close(pipe_fd[0]); // Close read end

        FILE *sendmail = fdopen(pipe_fd[1], "w");
        if (sendmail == NULL) {
            perror("fdopen failed");
            close(pipe_fd[1]);
            return 1;
        }

        // Write email headers and body
        fprintf(sendmail, "To: test@example.com\n");
        fprintf(sendmail, "From: sender@example.com\n");
        fprintf(sendmail, "Subject: Test Email\n");
        fprintf(sendmail, "\n");
        fprintf(sendmail, "This is a test email sent via vfork.\n");
        fprintf(sendmail, "Testing functionality.\n");
        fprintf(sendmail, ".\n");  // End of message marker
        fprintf(stdout, "vfork test: writing to pipe\n");
        fflush(stdout);

        // Close the pipe
        fclose(sendmail);

        // Wait for child process
        int status;
        if (waitpid(pid, &status, 0) == -1) {
            perror("waitpid failed");
            return 1;
        }

        if (WIFEXITED(status)) {
            printf("vfork test: Exit status: %d\n", WEXITSTATUS(status));
            return WEXITSTATUS(status);
        } else {
            printf("Child process did not exit normally\n");
            return 1;
        }
    }

    return 0;
}

// ============================================================================
// Popen test - spawns echo via shell using mypopen (this hangs due to bug)
// ============================================================================

int do_popen_test(void)
{
    FILE *sendmail;

    // Open process for writing using our custom popen
    // This spawns: ./main.wasm shell -c "./main.wasm echo"
    sendmail = mypopen("./main.wasm echo", "w");
    if (sendmail == NULL) {
        perror("mypopen failed");
        return 1;
    }

    // Write data
    fprintf(sendmail, "To: test@example.com\n");
    fprintf(sendmail, "From: sender@example.com\n");
    fprintf(sendmail, "Subject: Test Email\n");
    fprintf(sendmail, "\n");
    fprintf(sendmail, "This is a test email sent via popen.\n");
    fprintf(sendmail, "Testing sendmail functionality.\n");
    fprintf(sendmail, ".\n");  // End of message marker
    fprintf(stdout, "popen test: writing to pipe\n");
    fflush(stdout);
    fflush(sendmail);  // Flush before closing

    // Close the pipe and get the exit status
    // THIS HANGS BECAUSE FOR THE CHILD PROCESS STDIN NEVER REACHES EOF
    int status = mypclose(sendmail);
    if (status == -1) {
        perror("mypclose failed");
        return 1;
    }

    printf("popen test: Exit status: %d\n", status);
    return 0;
}

// ============================================================================
// Main dispatcher
// ============================================================================

int main(int argc, char *argv[])
{
    if (argc < 2) {
        // Default: run the popen test
        return do_popen_test();
    }

    if (strcmp(argv[1], "echo") == 0) {
        return do_echo();
    } else if (strcmp(argv[1], "shell") == 0) {
        return do_shell(argc, argv);
    } else if (strcmp(argv[1], "vfork") == 0) {
        return do_vfork_test();
    } else if (strcmp(argv[1], "popen") == 0) {
        return do_popen_test();
    } else {
        fprintf(stderr, "Unknown command: %s\n", argv[1]);
        fprintf(stderr, "Usage: main.wasm [echo|shell|vfork|popen]\n");
        return 1;
    }
}

