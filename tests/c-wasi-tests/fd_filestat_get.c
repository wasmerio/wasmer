#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

#define TESTFILE "fd_filestat_get_file"
#define TESTLINK "fd_filestat_get_link"
#define FILE_SIZE 1024

static void write_all(int fd, const void *buf, size_t len)
{
    const uint8_t *cursor = (const uint8_t *)buf;
    size_t remaining = len;

    while (remaining > 0) {
        ssize_t written = write(fd, cursor, remaining);
        assert(written > 0);
        cursor += (size_t)written;
        remaining -= (size_t)written;
    }
}

static void test_stdio_filestat(void)
{
    // From wasmtime p1_fd_filestat_get.rs.
    printf("Test 1: stdio filestat fields\n");
    __wasi_filestat_t stat;

    __wasi_errno_t err = __wasi_fd_filestat_get((__wasi_fd_t)STDIN_FILENO, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.size == 0);
    assert(stat.atim == 0);
    assert(stat.mtim == 0);
    assert(stat.ctim == 0);

    err = __wasi_fd_filestat_get((__wasi_fd_t)STDOUT_FILENO, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.size == 0);
    assert(stat.atim == 0);
    assert(stat.mtim == 0);
    assert(stat.ctim == 0);

    err = __wasi_fd_filestat_get((__wasi_fd_t)STDERR_FILENO, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.size == 0);
    assert(stat.atim == 0);
    assert(stat.mtim == 0);
    assert(stat.ctim == 0);
}

static void test_regular_file_and_link(void)
{
    // From LTP fstat02.c (size + nlink).
    printf("Test 2: regular file filestat + hard link\n");
    int fd = open(TESTFILE, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);

    uint8_t buffer[FILE_SIZE];
    memset(buffer, 'a', sizeof(buffer));
    write_all(fd, buffer, sizeof(buffer));

    int rc = link(TESTFILE, TESTLINK);
    assert(rc == 0);

    __wasi_filestat_t stat;
    __wasi_errno_t err = __wasi_fd_filestat_get((__wasi_fd_t)fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_REGULAR_FILE);
    assert(stat.size == FILE_SIZE);
    assert(stat.nlink == 2);

    int link_fd = open(TESTLINK, O_RDONLY);
    assert(link_fd >= 0);

    __wasi_filestat_t link_stat;
    err = __wasi_fd_filestat_get((__wasi_fd_t)link_fd, &link_stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(link_stat.filetype == __WASI_FILETYPE_REGULAR_FILE);
    assert(link_stat.size == FILE_SIZE);
    assert(link_stat.nlink == 2);
    assert(link_stat.ino == stat.ino);
    assert(link_stat.dev == stat.dev);

    close(link_fd);
    close(fd);
    assert(unlink(TESTLINK) == 0);
    assert(unlink(TESTFILE) == 0);
}

static void test_directory_filestat(void)
{
    // From wasmtime p1_dir_fd_op_failures.rs.
    printf("Test 3: directory filetype\n");
    int fd = open(".", O_RDONLY);
    assert(fd >= 0);

    __wasi_filestat_t stat;
    __wasi_errno_t err = __wasi_fd_filestat_get((__wasi_fd_t)fd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(stat.filetype == __WASI_FILETYPE_DIRECTORY);

    close(fd);
}

static void test_invalid_fd(void)
{
    // From LTP fstat03.c.
    printf("Test 4: invalid fd\n");
    __wasi_filestat_t stat;
    __wasi_errno_t err = __wasi_fd_filestat_get((__wasi_fd_t)9999, &stat);
    assert(err == __WASI_ERRNO_BADF);
}

static void test_invalid_pointer(void)
{
    // From LTP fstat03.c (invalid stat buffer).
    printf("Test 5: invalid stat buffer pointer\n");
    __wasi_filestat_t *bad_ptr = (__wasi_filestat_t *)(uintptr_t)0xFFFFFFFCu;
    __wasi_errno_t err = __wasi_fd_filestat_get((__wasi_fd_t)STDIN_FILENO, bad_ptr);
    printf("  err=%u\n", (unsigned)err);
    assert(err == __WASI_ERRNO_FAULT);
}

int main(void)
{
    test_stdio_filestat();
    test_regular_file_and_link();
    test_directory_filestat();
    test_invalid_fd();
    test_invalid_pointer();
    printf("All tests passed!\n");
    return 0;
}
