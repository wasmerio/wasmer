#include <assert.h>
#include <stdint.h>
#include <stdio.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

static void test_basic_create(void)
{
    printf("Test 1: epoll_create basic\n");

    __wasi_fd_t epfd = 0;
    __wasi_errno_t err = __wasi_epoll_create(&epfd);
    assert(err == __WASI_ERRNO_SUCCESS);

    __wasi_fdstat_t stat;
    err = __wasi_fd_fdstat_get(epfd, &stat);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert((stat.fs_rights_base & __WASI_RIGHTS_POLL_FD_READWRITE) ==
           __WASI_RIGHTS_POLL_FD_READWRITE);

    err = __wasi_fd_close(epfd);
    assert(err == __WASI_ERRNO_SUCCESS);
}

static void test_multiple_create_unique(void)
{
    printf("Test 2: epoll_create returns distinct fds\n");

    __wasi_fd_t a = 0;
    __wasi_fd_t b = 0;

    assert(__wasi_epoll_create(&a) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_epoll_create(&b) == __WASI_ERRNO_SUCCESS);
    assert(a != b);

    assert(__wasi_fd_close(a) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(b) == __WASI_ERRNO_SUCCESS);
}

static void test_invalid_pointer(void)
{
    printf("Test 3: epoll_create invalid pointer\n");

    __wasi_fd_t *bad_ptr = (__wasi_fd_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_epoll_create(bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    printf("WASIX epoll_create integration tests\n");
    test_basic_create();
    test_multiple_create_unique();
    test_invalid_pointer();
    printf("All tests passed!\n");
    return 0;
}
