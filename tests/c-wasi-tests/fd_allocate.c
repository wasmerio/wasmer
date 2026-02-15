#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

static off_t file_size(int fd) {
    struct stat st;
    assert(fstat(fd, &st) == 0);
    return st.st_size;
}

int main(void) {
    const char *path = "fd_allocate_basic";
    unlink(path);

    int fd = open(path, O_CREAT | O_TRUNC | O_RDWR, 0644);
    assert(fd >= 0);
    assert(file_size(fd) == 0);

    __wasi_errno_t err = __wasi_fd_allocate((__wasi_fd_t)fd, 0, 10);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(file_size(fd) == 10);

    err = __wasi_fd_allocate((__wasi_fd_t)fd, 0, 5);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(file_size(fd) == 10);

    close(fd);
    assert(unlink(path) == 0);

    printf("All tests passed!\n");
    return 0;
}
