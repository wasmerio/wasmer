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
    int fd = open("/tmp/test.txt", O_CREAT | O_RDWR | O_TRUNC, 0644);
    if (fd == -1) {
        perror("open");
        return 1;
    }
    debug_printf("open succeeded\n");

    // 1. The file needs to be unlinked
    int unlink_result = unlink("/tmp/test.txt");
    if (unlink_result == -1) {
        perror("unlink");
        return 1;
    }
    debug_printf("unlink succeeded\n");

    FILE* fp = fdopen(fd, "w+");
    if (fp == NULL) {
        perror("fdopen");
        return 1;
    }
    debug_printf("fdopen succeeded\n");

    // 2. The write must be larger than 1024 bytes. Smaller writes succeed for some reason.
    char memory_buffer[1025];
    ssize_t n = fwrite(memory_buffer, 1, 1025, fp);
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

    int close_result = close(fd);
    if (close_result == -1) {
        perror("close");
        return 1;
    }
    debug_printf("close succeeded\n");

    // 1. The file needs to be unlinked
    int unlink_result = unlink("/tmp/test.txt");
    if (unlink_result == -1) {
        perror("unlink");
        return 1;
    }
    debug_printf("unlink succeeded\n");

        // 1. The file needs to be unlinked
    int unlink_result2 = unlink("/tmp/test.txt");
    if (unlink_result2 == 0) {
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
    int unlink_result = unlink("/tmp/test.txt");
    if (unlink_result == -1) {
        perror("unlink");
        return 1;
    }
    debug_printf("unlink succeeded\n");

    int unlink_result2 = unlink("/tmp/test.txt");
    if (unlink_result2 == 0) {
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

    int close_result = close(fd);
    if (close_result == -1) {
        perror("close");
        return 1;
    }
    debug_printf("close succeeded\n");

    // 1. The file needs to be unlinked
    int unlink_result = unlink("/tmp/test.txt");
    if (unlink_result == -1) {
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
    int unlink_result = unlink("/tmp/test.txt");
    if (unlink_result == -1) {
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
    ssize_t n = write(fd, memory_buffer, WRITE_SIZE-1);
    if (n != WRITE_SIZE-1) {
        fprintf(stderr, "Expected to write %d bytes to first file, but wrote %zu\n", WRITE_SIZE-1, n);
        return 1;
    }

    char* memory_buffer2 = malloc(WRITE_SIZE);
    memset(memory_buffer2, 'B', WRITE_SIZE-1);
    ssize_t n2 = write(fd2, memory_buffer2, WRITE_SIZE-1);
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
            fprintf(stderr, "Expected to read 'A' from first file, but got different data\n");
            return 1;
        }
    }
    debug_printf("read from first file succeeded and data is correct\n");

    ssize_t read_size2 = read(fd2, memory_buffer, WRITE_SIZE-1);
    if (read_size2 != WRITE_SIZE-1) {
        fprintf(stderr, "Expected to read %d bytes from second file, but got %zd\n", WRITE_SIZE-1, read_size2);
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
    int unlink_result = unlink("/tmp/test.txt");
    if (unlink_result == -1) {
        perror("unlink");
        return 1;
    }
    debug_printf("unlink succeeded\n");

    int WRITE_SIZE = 5000;

    char* memory_buffer = malloc(WRITE_SIZE);
    memset(memory_buffer, 'A', WRITE_SIZE-1);
    ssize_t n = write(fd, memory_buffer, WRITE_SIZE-1);
    if (n != WRITE_SIZE-1) {
        fprintf(stderr, "Expected to write %d bytes to first file, but wrote %zu\n", WRITE_SIZE-1, n);
        return 1;
    }

    char* memory_buffer2 = malloc(WRITE_SIZE);
    memset(memory_buffer2, 'B', WRITE_SIZE-1);
    ssize_t n2 = write(fd2, memory_buffer2, WRITE_SIZE-1);
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

// Test basic directory removal with unlinkat: empty directory removal and double removal
int test_rmdir_basic() {
    // Test 1: Remove an empty directory with unlinkat (requires AT_REMOVEDIR flag)
    int result = mkdir("/tmp/test_dir", 0755);
    if (result == -1) {
        perror("mkdir");
        return 1;
    }
    debug_printf("mkdir succeeded\n");

    result = unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);
    if (result == -1) {
        perror("unlinkat with AT_REMOVEDIR");
        return 1;
    }
    debug_printf("unlinkat with AT_REMOVEDIR succeeded\n");

    // Test 2: Second unlinkat should fail with ENOENT
    result = unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);
    if (result == 0) {
        fprintf(stderr, "Expected second unlinkat to fail, but it succeeded\n");
        return 1;
    }
    if (errno != ENOENT) {
        fprintf(stderr, "Expected errno ENOENT, but got %d\n", errno);
        return 1;
    }
    debug_printf("second unlinkat failed as expected with ENOENT\n");

    return 0;
}

// Test that we cannot remove a non-empty directory
int test_rmdir_non_empty() {
    int result = mkdir("/tmp/test_dir", 0755);
    if (result == -1) {
        perror("mkdir");
        return 1;
    }
    debug_printf("mkdir succeeded\n");

    // Create a file inside the directory
    int fd = open("/tmp/test_dir/file.txt", O_CREAT | O_WRONLY, 0644);
    if (fd == -1) {
        perror("open");
        return 1;
    }
    close(fd);
    debug_printf("file created\n");

    // Attempt to remove the non-empty directory with unlinkat
    result = unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);
    if (result == 0) {
        fprintf(stderr, "Expected unlinkat to fail on non-empty directory, but it succeeded\n");
        return 1;
    }
    if (errno != ENOTEMPTY && errno != EEXIST) {
        fprintf(stderr, "Expected errno ENOTEMPTY or EEXIST, but got %d\n", errno);
        return 1;
    }
    debug_printf("unlinkat failed as expected with errno %d\n", errno);

    // Cleanup
    unlink("/tmp/test_dir/file.txt");
    unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);

    return 0;
}

// Test error cases: unlink() on directory and rmdir() on file
int test_rmdir_unlink_errors() {
    // Test 1: unlink() should fail on directory with EISDIR
    int result = mkdir("/tmp/test_dir", 0755);
    if (result == -1) {
        perror("mkdir");
        return 1;
    }
    debug_printf("mkdir succeeded\n");

    result = unlink("/tmp/test_dir");
    if (result == 0) {
        fprintf(stderr, "Expected unlink to fail on directory, but it succeeded\n");
        return 1;
    }
    if (errno != EISDIR) {
        fprintf(stderr, "Expected errno EISDIR, but got %d\n", errno);
        return 1;
    }
    debug_printf("unlink failed as expected with EISDIR\n");

    unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);

    // Test 2: unlinkat with AT_REMOVEDIR should fail on file with ENOTDIR
    int fd = open("/tmp/test_file.txt", O_CREAT | O_WRONLY | O_TRUNC, 0644);
    if (fd == -1) {
        perror("open");
        return 1;
    }
    close(fd);
    debug_printf("file created\n");

    result = unlinkat(AT_FDCWD, "/tmp/test_file.txt", AT_REMOVEDIR);
    if (result == 0) {
        fprintf(stderr, "Expected unlinkat with AT_REMOVEDIR to fail on file, but it succeeded\n");
        return 1;
    }
    if (errno != ENOTDIR) {
        fprintf(stderr, "Expected errno ENOTDIR, but got %d\n", errno);
        return 1;
    }
    debug_printf("unlinkat with AT_REMOVEDIR failed as expected with ENOTDIR\n");

    unlink("/tmp/test_file.txt");

    return 0;
}

// Test behavior after directory removal: access and recreation
int test_rmdir_after_behavior() {
    // Test 1: Cannot access directory after removal
    int result = mkdir("/tmp/test_dir", 0755);
    if (result == -1) {
        perror("mkdir");
        return 1;
    }
    debug_printf("mkdir succeeded\n");

    result = unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);
    if (result == -1) {
        perror("unlinkat");
        return 1;
    }
    debug_printf("unlinkat succeeded\n");

    int fd = open("/tmp/test_dir/file.txt", O_CREAT | O_WRONLY, 0644);
    if (fd != -1) {
        fprintf(stderr, "Expected open to fail in removed directory, but it succeeded\n");
        close(fd);
        return 1;
    }
    if (errno != ENOENT) {
        fprintf(stderr, "Expected errno ENOENT, but got %d\n", errno);
        return 1;
    }
    debug_printf("open failed as expected with ENOENT\n");

    // Test 2: Can create new independent directory with same name
    result = mkdir("/tmp/test_dir", 0755);
    if (result == -1) {
        perror("mkdir after unlinkat");
        return 1;
    }
    debug_printf("mkdir after unlinkat succeeded\n");

    fd = open("/tmp/test_dir/file.txt", O_CREAT | O_RDWR, 0644);
    if (fd == -1) {
        perror("open in new directory");
        return 1;
    }
    write(fd, "NEW", 3);
    lseek(fd, 0, SEEK_SET);
    
    char buf[10];
    ssize_t n = read(fd, buf, 3);
    if (n != 3 || memcmp(buf, "NEW", 3) != 0) {
        fprintf(stderr, "Expected to read 'NEW' from new directory\n");
        return 1;
    }
    close(fd);
    debug_printf("verified new directory is independent\n");

    // Cleanup
    unlink("/tmp/test_dir/file.txt");
    unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);

    return 0;
}

// Test nested directory removal
int test_rmdir_nested() {
    int result = mkdir("/tmp/test_dir", 0755);
    if (result == -1) {
        perror("mkdir");
        return 1;
    }

    result = mkdir("/tmp/test_dir/subdir", 0755);
    if (result == -1) {
        perror("mkdir subdir");
        return 1;
    }
    debug_printf("nested mkdir succeeded\n");

    // Cannot remove parent while child exists
    result = unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);
    if (result == 0) {
        fprintf(stderr, "Expected unlinkat to fail on directory with subdirectory\n");
        return 1;
    }
    debug_printf("unlinkat parent failed as expected\n");

    // Remove child first
    result = unlinkat(AT_FDCWD, "/tmp/test_dir/subdir", AT_REMOVEDIR);
    if (result == -1) {
        perror("unlinkat subdir");
        return 1;
    }
    debug_printf("unlinkat subdir succeeded\n");

    // Now remove parent
    result = unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);
    if (result == -1) {
        perror("unlinkat parent");
        return 1;
    }
    debug_printf("rmdir parent succeeded\n");

    return 0;
}

// Test unlinkat with directory file descriptors (dirfd)
int test_unlinkat_with_dirfd() {
    // Create test directory structure
    int result = mkdir("/tmp/test_parent", 0755);
    if (result == -1) {
        perror("mkdir parent");
        return 1;
    }

    result = mkdir("/tmp/test_parent/subdir", 0755);
    if (result == -1) {
        perror("mkdir subdir");
        return 1;
    }

    // Create files in both directories
    int fd1 = open("/tmp/test_parent/file1.txt", O_CREAT | O_WRONLY, 0644);
    if (fd1 == -1) {
        perror("open file1");
        return 1;
    }
    write(fd1, "FILE1", 5);
    close(fd1);

    int fd2 = open("/tmp/test_parent/subdir/file2.txt", O_CREAT | O_WRONLY, 0644);
    if (fd2 == -1) {
        perror("open file2");
        return 1;
    }
    write(fd2, "FILE2", 5);
    close(fd2);
    debug_printf("created directory structure\n");

    // Open parent directory as dirfd
    int dirfd = open("/tmp/test_parent", O_RDONLY | O_DIRECTORY);
    if (dirfd == -1) {
        perror("open parent directory");
        return 1;
    }
    debug_printf("opened parent directory as dirfd\n");

    // Test 1: Unlink file using dirfd
    result = unlinkat(dirfd, "file1.txt", 0);
    if (result == -1) {
        perror("unlinkat file1.txt with dirfd");
        return 1;
    }
    debug_printf("unlinkat file with dirfd succeeded\n");

    // Verify file is gone
    int test_fd = openat(dirfd, "file1.txt", O_RDONLY);
    if (test_fd != -1) {
        fprintf(stderr, "Expected file1.txt to be gone after unlinkat\n");
        close(test_fd);
        return 1;
    }

    // Test 2: Unlink file in subdirectory using dirfd
    result = unlinkat(dirfd, "subdir/file2.txt", 0);
    if (result == -1) {
        perror("unlinkat subdir/file2.txt with dirfd");
        return 1;
    }
    debug_printf("unlinkat file in subdirectory with dirfd succeeded\n");

    // Test 3: Remove empty subdirectory using dirfd
    result = unlinkat(dirfd, "subdir", AT_REMOVEDIR);
    if (result == -1) {
        perror("unlinkat subdir with AT_REMOVEDIR and dirfd");
        return 1;
    }
    debug_printf("unlinkat subdirectory with dirfd succeeded\n");

    close(dirfd);

    // Cleanup
    unlinkat(AT_FDCWD, "/tmp/test_parent", AT_REMOVEDIR);

    return 0;
}

// Test that unlinkat without AT_REMOVEDIR flag fails on directories
int test_unlinkat_dir_without_flag() {
    // Create a directory
    int result = mkdir("/tmp/test_dir", 0755);
    if (result == -1) {
        perror("mkdir");
        return 1;
    }
    debug_printf("mkdir succeeded\n");

    // Try to unlinkat the directory without AT_REMOVEDIR flag
    result = unlinkat(AT_FDCWD, "/tmp/test_dir", 0);
    if (result == 0) {
        fprintf(stderr, "Expected unlinkat without AT_REMOVEDIR to fail on directory, but it succeeded\n");
        return 1;
    }
    if (errno != EISDIR) {
        fprintf(stderr, "Expected errno EISDIR, but got %d\n", errno);
        return 1;
    }
    debug_printf("unlinkat without AT_REMOVEDIR failed as expected with EISDIR\n");

    // Cleanup
    unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);

    return 0;
}

// Test that unlink() fails on directories with EISDIR
int test_unlink_dir_fails() {
    // Create a directory
    int result = mkdir("/tmp/test_dir", 0755);
    if (result == -1) {
        perror("mkdir");
        return 1;
    }
    debug_printf("mkdir succeeded\n");

    // Try to unlink the directory
    result = unlink("/tmp/test_dir");
    if (result == 0) {
        fprintf(stderr, "Expected unlink to fail on directory, but it succeeded\n");
        return 1;
    }
    if (errno != EISDIR) {
        fprintf(stderr, "Expected errno EISDIR, but got %d\n", errno);
        return 1;
    }
    debug_printf("unlink failed as expected with EISDIR\n");

    // Cleanup
    unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);

    return 0;
}

// Test unlinking a directory while it has an open dirfd
// Similar to test_unlink_with_two_fds but for directories
int test_unlink_dir_with_open_dirfd() {
    // Create a directory
    int result = mkdir("/tmp/test_dir", 0755);
    if (result == -1) {
        perror("mkdir");
        return 1;
    }

    // Create a file inside the directory
    int file_fd = open("/tmp/test_dir/test.txt", O_CREAT | O_RDWR, 0644);
    if (file_fd == -1) {
        perror("open file in directory");
        return 1;
    }
    write(file_fd, "CONTENT", 7);
    close(file_fd);
    debug_printf("created directory with file\n");

    // Open the directory with O_DIRECTORY to get a dirfd
    int dirfd = open("/tmp/test_dir", O_RDONLY | O_DIRECTORY);
    if (dirfd == -1) {
        perror("open directory");
        return 1;
    }
    debug_printf("opened directory as dirfd\n");

    // Remove the file inside the directory
    result = unlink("/tmp/test_dir/test.txt");
    if (result == -1) {
        perror("unlink file");
        return 1;
    }

    // Now unlink the directory (should succeed even with open dirfd)
    result = unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);
    if (result == -1) {
        perror("unlinkat directory with open dirfd");
        return 1;
    }
    debug_printf("unlinkat directory succeeded with open dirfd\n");

    // The dirfd should still be valid and usable
    // Try to list directory contents with fstat
    struct stat st;
    result = fstat(dirfd, &st);
    if (result == -1) {
        perror("fstat on dirfd after directory unlink");
        return 1;
    }
    debug_printf("fstat on dirfd succeeded after unlink\n");

    // Creating a new file through the old dirfd should fail
    int new_fd = openat(dirfd, "newfile.txt", O_CREAT | O_WRONLY, 0644);
    if (new_fd != -1) {
        fprintf(stderr, "Expected openat to fail on unlinked directory dirfd, but it succeeded\n");
        close(new_fd);
        return 1;
    }
    debug_printf("openat on unlinked directory dirfd failed as expected\n");

    // Cannot access the directory by path anymore
    int test_fd = open("/tmp/test_dir", O_RDONLY | O_DIRECTORY);
    if (test_fd != -1) {
        fprintf(stderr, "Expected open to fail on unlinked directory, but it succeeded\n");
        close(test_fd);
        return 1;
    }
    debug_printf("open by path failed as expected\n");

    // Close the dirfd
    close(dirfd);

    // Verify we can create a new directory with the same name
    result = mkdir("/tmp/test_dir", 0755);
    if (result == -1) {
        perror("mkdir after unlinking directory with open dirfd");
        return 1;
    }
    debug_printf("created new directory with same name\n");

    // Cleanup
    unlinkat(AT_FDCWD, "/tmp/test_dir", AT_REMOVEDIR);

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
    if (!strcmp(argv[1], "test_rmdir_basic"))
    {
        return test_rmdir_basic();
    }
    if (!strcmp(argv[1], "test_rmdir_non_empty"))
    {
        return test_rmdir_non_empty();
    }
    if (!strcmp(argv[1], "test_rmdir_unlink_errors"))
    {
        return test_rmdir_unlink_errors();
    }
    if (!strcmp(argv[1], "test_rmdir_after_behavior"))
    {
        return test_rmdir_after_behavior();
    }
    if (!strcmp(argv[1], "test_rmdir_nested"))
    {
        return test_rmdir_nested();
    }
    if (!strcmp(argv[1], "test_unlinkat_with_dirfd"))
    {
        return test_unlinkat_with_dirfd();
    }
    if (!strcmp(argv[1], "test_unlinkat_dir_without_flag"))
    {
        return test_unlinkat_dir_without_flag();
    }
    if (!strcmp(argv[1], "test_unlink_dir_fails"))
    {
        return test_unlink_dir_fails();
    }
    if (!strcmp(argv[1], "test_unlink_dir_with_open_dirfd"))
    {
        return test_unlink_dir_with_open_dirfd();
    }
    fprintf(stderr, "Unknown subtest %s\n", argv[1]);
    return -1;
}