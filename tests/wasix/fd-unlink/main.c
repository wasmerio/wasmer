#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/stat.h>
// #include <libexplain/read.h>

#define debug_printf(...)
// Uncomment to enable debug prints
// #define debug_printf(...) printf(__VA_ARGS__)

int test_unlink() {
    int fd = open("/tmp/test.txt", O_CREAT | O_WRONLY | O_TRUNC, 0644);
    if (fd == -1) {
        perror("open");
        return 1;
    }
    debug_printf("open succeeded\n");

    // 1. The file needs to be unlinked
    unlink("/tmp/test.txt");
    if (errno != 0) {
        perror("unlink");
        return 1;
    }
    debug_printf("unlink succeeded\n");

    FILE* fp = fdopen(fd, "wr");
    if (fp == NULL) {
        perror("fdopen");
        return 1;
    }
    debug_printf("fdopen succeeded\n");

    // 2. The write must be larger than 1024 bytes. Smaller writes succeed for some reason.
    char memory_buffer[1025];
    size_t n = fwrite(memory_buffer, 1, 1025, fp);
    if (ferror(fp)) {
        perror("fwrite");
        return 1;
    }
    debug_printf("writing succeeded\n");

    close(fd);
    return 0;
}

// Verify that unlinking a file twice results in an error the second time
int test_unlink_twice() {
    int fd = open("/tmp/test.txt", O_CREAT | O_WRONLY | O_TRUNC, 0644);
    if (fd == -1) {
        perror("open");
        return 1;
    }
    debug_printf("open succeeded\n");

    close(fd);
    if (errno != 0) {
        perror("close");
        return 1;
    }
    debug_printf("close succeeded\n");

    // 1. The file needs to be unlinked
    unlink("/tmp/test.txt");
    if (errno != 0) {
        perror("unlink");
        return 1;
    }
    debug_printf("unlink succeeded\n");

        // 1. The file needs to be unlinked
    unlink("/tmp/test.txt");
    if (errno == 0) {
        fprintf(stderr, "Expected error on second unlink, but got none\n");
        return 1;
    }
    debug_printf("Second unlink failed as expected\n");

    return 0;
}

// Verify that unlinking a file twice results in an error the second time
//
// Even if we still have an open file descriptor to it.
int test_unlink_twice_with_open_fd() {
    int fd = open("/tmp/test.txt", O_CREAT | O_WRONLY | O_TRUNC, 0644);
    if (fd == -1) {
        perror("open");
        return 1;
    }
    debug_printf("open succeeded\n");

    // 1. The file needs to be unlinked
    unlink("/tmp/test.txt");
    if (errno != 0) {
        perror("unlink");
        return 1;
    }
    debug_printf("unlink succeeded\n");

    unlink("/tmp/test.txt");
    if (errno == 0) {
        fprintf(stderr, "Expected error on second unlink, but got none\n");
        return 1;
    }
    debug_printf("Second unlink failed as expected\n");

    return 0;
}

// Verify that we can't open a file after unlinking it
int test_open_after_unlink() {
    int fd = open("/tmp/test.txt", O_CREAT | O_WRONLY | O_TRUNC, 0644);
    if (fd == -1) {
        perror("open");
        return 1;
    }
    debug_printf("open succeeded\n");

    close(fd);
    if (errno != 0) {
        perror("close");
        return 1;
    }
    debug_printf("close succeeded\n");

    // 1. The file needs to be unlinked
    unlink("/tmp/test.txt");
    if (errno != 0) {
        perror("unlink");
        return 1;
    }
    debug_printf("unlink succeeded\n");

    int fd2 = open("/tmp/test.txt", O_WRONLY | O_TRUNC, 0644);
    if (fd2 != -1) {
        fprintf(stderr, "Expected open to fail after unlink, but it succeeded\n");
        return 1;
    }
    debug_printf("open after unlink failed as expected\n");

    return 0;
}

// Create a file, write to it, close it, unlink it, and then create a new file with the same name and write to it.
//
// While both files were created with the same name, they should be different files.
// Reading from the second file should not show the contents written to the first file.
int test_new_file_after_unlink_is_new_file() {
    int fd = open("/tmp/test.txt", O_CREAT | O_RDWR | O_TRUNC, 0644);
    if (fd == -1) {
        perror("open");
        return 1;
    }
    debug_printf("open succeeded\n");

    // 1. The file needs to be unlinked
    unlink("/tmp/test.txt");
    if (errno != 0) {
        perror("unlink");
        return 1;
    }
    debug_printf("unlink succeeded\n");

    int fd2 = open("/tmp/test.txt", O_CREAT | O_RDWR | O_TRUNC, 0644);
    if (fd2 == -1) {
        perror("second open");
        return 1;
    }
    debug_printf("second open succeeded\n");

    int WRITE_SIZE = 5000;

    char* memory_buffer = malloc(WRITE_SIZE);
    memset(memory_buffer, 'A', WRITE_SIZE-1);
    size_t n = write(fd, memory_buffer, WRITE_SIZE-1);
    if (n != WRITE_SIZE-1) {
        fprintf(stderr, "Expected to write %d bytes to first file, but wrote %zu\n", WRITE_SIZE-1, n);
        return 1;
    }

    char* memory_buffer2 = malloc(WRITE_SIZE);
    memset(memory_buffer2, 'B', WRITE_SIZE-1);
    size_t n2 = write(fd2, memory_buffer2, WRITE_SIZE-1);
    if (n2 != WRITE_SIZE-1) {
        fprintf(stderr, "Expected to write %d bytes to second file, but wrote %zu\n", WRITE_SIZE-1, n2);
        return 1;
    }

    fsync(fd);
    fsync(fd2);

    lseek(fd, 0, SEEK_SET);
    lseek(fd2, 0, SEEK_SET);

    ssize_t read_size = read(fd, memory_buffer2, WRITE_SIZE-1);
    if (read_size != WRITE_SIZE-1) {
        // fprintf(stderr, "%s\n", explain_read(fd, memory_buffer2, WRITE_SIZE-1));
        fprintf(stderr, "Expected to read %d bytes from first file, but got %zd\n", WRITE_SIZE-1, read_size);
        return 1;
    }
    for (int i = 0; i < WRITE_SIZE-1; i++) {
        if (memory_buffer2[i] != 'A') {
            fprintf(stderr, "Expected to read 'B' from first file, but got different data\n");
            return 1;
        }
    }
    debug_printf("read from first file succeeded and data is correct\n");

    ssize_t read_size2 = read(fd2, memory_buffer, WRITE_SIZE-1);
    if (read_size2 != WRITE_SIZE-1) {
        fprintf(stderr, "Expected to read %d bytes from second file, but got %zd\n", WRITE_SIZE-1, read_size);
        return 1;
    }
    for (int i = 0; i < WRITE_SIZE-1; i++) {
        if (memory_buffer[i] != 'B') {
            fprintf(stderr, "Expected to read 'B' from second file, but got different data\n");
            return 1;
        }
    }
    debug_printf("read from second file succeeded and data is correct\n");

    return 0;
}

// Open the same file twice with two different file descriptors. Unlink the file.
//
// Close the first file descriptor, verify that the second file descriptor is still valid and can read/write data.
int test_unlink_with_two_fds() {
    int fd = open("/tmp/test.txt", O_CREAT | O_RDWR | O_TRUNC, 0644);
    if (fd == -1) {
        perror("open");
        return 1;
    }
    debug_printf("open succeeded\n");

    int fd2 = open("/tmp/test.txt", O_CREAT | O_RDWR | O_TRUNC, 0644);
    if (fd2 == -1) {
        perror("open");
        return 1;
    }
    debug_printf("open succeeded\n");

    if (fd == fd2) {
        fprintf(stderr, "Expected two different file descriptors, but got the same\n");
        return 1;
    }

    // 1. The file needs to be unlinked
    unlink("/tmp/test.txt");
    if (errno != 0) {
        perror("unlink");
        return 1;
    }
    debug_printf("unlink succeeded\n");

    int WRITE_SIZE = 5000;

    char* memory_buffer = malloc(WRITE_SIZE);
    memset(memory_buffer, 'A', WRITE_SIZE-1);
    size_t n = write(fd, memory_buffer, WRITE_SIZE-1);
    if (n != WRITE_SIZE-1) {
        fprintf(stderr, "Expected to write %d bytes to first file, but wrote %zu\n", WRITE_SIZE-1, n);
        return 1;
    }

    char* memory_buffer2 = malloc(WRITE_SIZE);
    memset(memory_buffer2, 'B', WRITE_SIZE-1);
    size_t n2 = write(fd2, memory_buffer2, WRITE_SIZE-1);
    if (n2 != WRITE_SIZE-1) {
        fprintf(stderr, "Expected to write %d bytes to second file, but wrote %zu\n", WRITE_SIZE-1, n2);
        return 1;
    }

    close(fd2);

    fsync(fd);
    lseek(fd, 0, SEEK_SET);

    ssize_t read_size = read(fd2, memory_buffer2, WRITE_SIZE-1);
    if (read_size != -1) {
        // fprintf(stderr, "%s\n", explain_read(fd, memory_buffer2, WRITE_SIZE-1));
        fprintf(stderr, "Expected read from fd2 to fail, as fd2 was closed");
        return 1;
    }

    ssize_t read_size2 = read(fd, memory_buffer, WRITE_SIZE-1);
    if (read_size2 != WRITE_SIZE-1) {
        fprintf(stderr, "Expected to read %d bytes from fd, but got %zd\n", WRITE_SIZE-1, read_size);
        return 1;
    }
    for (int i = 0; i < WRITE_SIZE-1; i++) {
        if (memory_buffer[i] != 'B') {
            fprintf(stderr, "Expected to read 'B' from fd, but got different data\n");
            return 1;
        }
    }
    debug_printf("read from fd succeeded and data is correct\n");

    return 0;
}


int main(int argc, char **argv)
{
    if (argc < 2)
    {
        return -1;
    }

    if (!strcmp(argv[1], "test_unlink"))
    {
        return test_unlink();
    }
    if (!strcmp(argv[1], "test_unlink_twice"))
    {
        return test_unlink_twice();
    }
    if (!strcmp(argv[1], "test_unlink_twice_with_open_fd"))
    {
        return test_unlink_twice_with_open_fd();
    }
    if (!strcmp(argv[1], "test_open_after_unlink"))
    {
        return test_open_after_unlink();
    }
    if (!strcmp(argv[1], "test_new_file_after_unlink_is_new_file"))
    {
        return test_new_file_after_unlink_is_new_file();
    }
    if (!strcmp(argv[1], "test_unlink_with_two_fds"))
    {
        return test_unlink_with_two_fds();
    }
    fprintf(stderr, "Unknown subtest %s\n", argv[1]);
    return -1;
}