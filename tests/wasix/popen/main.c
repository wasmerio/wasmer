// Test file for popen stdin close issue
// Verifies that pipe2(O_CLOEXEC) correctly closes fds after posix_spawn
//
// Contains:
// - echo: reads stdin until EOF, writes to stdout
// - shell: minimal shell that supports "sh -c <command>"
// - posix_spawn_direct: baseline test using explicit addclose
// - pipe2_cloexec: tests pipe2+O_CLOEXEC without addclose (the fix)
// - popen: tests mypopen implementation

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
			// Use ./main.wasm shell instead of /bin/sh
			if (!(e = posix_spawn(&pid, "./main.wasm", &fa, 0,
			    (char *[]){ "./main.wasm", "shell", "-c", (char *)cmd, 0 }, environ))) {
				posix_spawn_file_actions_destroy(&fa);
				g_popen_pid = pid;
				if (!strchr(mode, 'e'))
					fcntl(p[op], F_SETFD, 0);
				close(p[1-op]);
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

int do_echo(void)
{
	char buffer[4096];
	size_t bytes_read;

	while ((bytes_read = fread(buffer, 1, sizeof(buffer), stdin)) > 0) {
		fwrite(buffer, 1, bytes_read, stdout);
	}
	fflush(stdout);
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
// posix_spawn direct test - baseline using explicit addclose (always works)
// ============================================================================

int do_posix_spawn_direct_test(void)
{
	int pipe_fd[2];
	pid_t pid;
	posix_spawn_file_actions_t fa;
	int e;

	__wasilibc_ensure_environ();

	if (pipe(pipe_fd) == -1) {
		perror("pipe failed");
		return 1;
	}

	e = posix_spawn_file_actions_init(&fa);
	if (e) {
		perror("posix_spawn_file_actions_init failed");
		return 1;
	}

	// Redirect stdin of child to read end of pipe
	e = posix_spawn_file_actions_adddup2(&fa, pipe_fd[0], STDIN_FILENO);
	if (e) {
		perror("posix_spawn_file_actions_adddup2 failed");
		return 1;
	}

	// Explicitly close both ends in child - this always works
	posix_spawn_file_actions_addclose(&fa, pipe_fd[0]);
	posix_spawn_file_actions_addclose(&fa, pipe_fd[1]);

	char *args[] = {"./main.wasm", "echo", NULL};
	e = posix_spawn(&pid, "./main.wasm", &fa, NULL, args, environ);
	posix_spawn_file_actions_destroy(&fa);

	if (e) {
		fprintf(stderr, "posix_spawn failed: %s\n", strerror(e));
		return 1;
	}

	close(pipe_fd[0]);

	FILE *out = fdopen(pipe_fd[1], "w");
	if (out == NULL) {
		perror("fdopen failed");
		close(pipe_fd[1]);
		return 1;
	}

	fprintf(out, "posix_spawn_direct: test data\n");
	fclose(out);

	int status;
	if (waitpid(pid, &status, 0) == -1) {
		perror("waitpid failed");
		return 1;
	}

	if (WIFEXITED(status)) {
		printf("posix_spawn_direct: exit status %d\n", WEXITSTATUS(status));
		return WEXITSTATUS(status);
	} else {
		printf("posix_spawn_direct: child did not exit normally\n");
		return 1;
	}
}

// ============================================================================
// pipe2+O_CLOEXEC test - tests that O_CLOEXEC closes fds without addclose
// This is the key test - relies on pipe2(O_CLOEXEC) working correctly
// ============================================================================

int do_pipe2_cloexec_test(void)
{
	int pipe_fd[2];
	pid_t pid;
	posix_spawn_file_actions_t fa;
	int e;

	__wasilibc_ensure_environ();

	// Use pipe2 with O_CLOEXEC - this should auto-close both ends in child
	if (pipe2(pipe_fd, O_CLOEXEC) == -1) {
		perror("pipe2 failed");
		return 1;
	}

	e = posix_spawn_file_actions_init(&fa);
	if (e) {
		perror("posix_spawn_file_actions_init failed");
		return 1;
	}

	// Only adddup2 - NO addclose. Relies on O_CLOEXEC to close pipe ends.
	e = posix_spawn_file_actions_adddup2(&fa, pipe_fd[0], STDIN_FILENO);
	if (e) {
		perror("posix_spawn_file_actions_adddup2 failed");
		return 1;
	}

	char *args[] = {"./main.wasm", "shell", "-c", "./main.wasm echo", NULL};
	e = posix_spawn(&pid, "./main.wasm", &fa, NULL, args, environ);
	posix_spawn_file_actions_destroy(&fa);

	if (e) {
		fprintf(stderr, "posix_spawn failed: %s\n", strerror(e));
		return 1;
	}

	close(pipe_fd[0]);

	FILE *out = fdopen(pipe_fd[1], "w");
	if (out == NULL) {
		perror("fdopen failed");
		close(pipe_fd[1]);
		return 1;
	}

	fprintf(out, "pipe2_cloexec: test data\n");
	fclose(out);

	int status;
	if (waitpid(pid, &status, 0) == -1) {
		perror("waitpid failed");
		return 1;
	}

	if (WIFEXITED(status)) {
		printf("pipe2_cloexec: exit status %d\n", WEXITSTATUS(status));
		return WEXITSTATUS(status);
	} else {
		printf("pipe2_cloexec: child did not exit normally\n");
		return 1;
	}
}

// ============================================================================
// Popen test - tests the mypopen implementation
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

	fprintf(sendmail, "popen: test data\n");
	fflush(sendmail);

	int status = mypclose(sendmail);
	if (status == -1) {
		perror("mypclose failed");
		return 1;
	}

	printf("popen: exit status %d\n", status);
	return 0;
}

// ============================================================================
// Main dispatcher
// ============================================================================

int main(int argc, char *argv[])
{
	if (argc < 2) {
		fprintf(stderr, "Usage: main.wasm <command>\n");
		fprintf(stderr, "Commands: echo, shell, posix_spawn_direct, pipe2_cloexec, popen\n");
		return 1;
	}

	if (strcmp(argv[1], "echo") == 0) {
		return do_echo();
	} else if (strcmp(argv[1], "shell") == 0) {
		return do_shell(argc, argv);
	} else if (strcmp(argv[1], "posix_spawn_direct") == 0) {
		return do_posix_spawn_direct_test();
	} else if (strcmp(argv[1], "pipe2_cloexec") == 0) {
		return do_pipe2_cloexec_test();
	} else if (strcmp(argv[1], "popen") == 0) {
		return do_popen_test();
	} else {
		fprintf(stderr, "Unknown command: %s\n", argv[1]);
		fprintf(stderr, "Commands: echo, shell, posix_spawn_direct, pipe2_cloexec, popen\n");
		return 1;
	}
}
