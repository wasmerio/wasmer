#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

static __wasi_fd_t create_epoll_fd(void)
{
    __wasi_fd_t epfd = 0;
    __wasi_errno_t err = __wasi_epoll_create(&epfd);
    assert(err == __WASI_ERRNO_SUCCESS);
    return epfd;
}

static void create_pipe(__wasi_fd_t *read_end, __wasi_fd_t *write_end)
{
    __wasi_errno_t err = __wasi_fd_pipe(read_end, write_end);
    assert(err == __WASI_ERRNO_SUCCESS);
}

static __wasi_epoll_event_t make_event(__wasi_epoll_type_t events, __wasi_fd_t fd)
{
    __wasi_epoll_event_t ev;
    memset(&ev, 0, sizeof(ev));
    ev.events = events;
    ev.data.fd = fd;
    return ev;
}

static void test_basic_add_del_duplicate(void)
{
    printf("Test 1: add/del and duplicate add\n");

    __wasi_fd_t epfd = create_epoll_fd();
    __wasi_fd_t rfd = 0;
    __wasi_fd_t wfd = 0;
    create_pipe(&rfd, &wfd);

    __wasi_epoll_event_t ev = make_event(__WASI_EPOLL_TYPE_EPOLLIN, rfd);
    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_ADD, rfd, &ev) == __WASI_ERRNO_SUCCESS);

    __wasi_errno_t err = __wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_ADD, rfd, &ev);
    assert(err == __WASI_ERRNO_EXIST);

    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_DEL, rfd, &ev) == __WASI_ERRNO_SUCCESS);

    assert(__wasi_fd_close(rfd) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(wfd) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(epfd) == __WASI_ERRNO_SUCCESS);
}

static void test_mod_and_wait(void)
{
    printf("Test 2: mod + wait integration\n");

    __wasi_fd_t epfd = create_epoll_fd();
    __wasi_fd_t rfd = 0;
    __wasi_fd_t wfd = 0;
    create_pipe(&rfd, &wfd);

    __wasi_epoll_event_t ev = make_event(__WASI_EPOLL_TYPE_EPOLLIN, rfd);
    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_ADD, rfd, &ev) == __WASI_ERRNO_SUCCESS);

    ev.events = (__wasi_epoll_type_t)(__WASI_EPOLL_TYPE_EPOLLIN | __WASI_EPOLL_TYPE_EPOLLET);
    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_MOD, rfd, &ev) == __WASI_ERRNO_SUCCESS);

    const char payload[] = "epoll";
    __wasi_size_t written = 0;
    assert(__wasi_fd_write(wfd, (const __wasi_ciovec_t[]){{(const void *)payload, sizeof(payload)}}, 1, &written) == __WASI_ERRNO_SUCCESS);
    assert(written == sizeof(payload));

    __wasi_epoll_event_t events[2];
    __wasi_size_t n = 0;
    assert(__wasi_epoll_wait(epfd, events, 2, 1000000000ull, &n) == __WASI_ERRNO_SUCCESS);
    assert(n >= 1);
    assert(events[0].data.fd == rfd);
    assert((events[0].events & __WASI_EPOLL_TYPE_EPOLLIN) != 0);

    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_DEL, rfd, &ev) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(rfd) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(wfd) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(epfd) == __WASI_ERRNO_SUCCESS);
}

static void test_invalid_args(void)
{
    printf("Test 3: invalid args\n");

    __wasi_fd_t epfd = create_epoll_fd();
    __wasi_fd_t rfd = 0;
    __wasi_fd_t wfd = 0;
    create_pipe(&rfd, &wfd);

    __wasi_epoll_event_t ev = make_event(__WASI_EPOLL_TYPE_EPOLLIN, rfd);

    assert(__wasi_epoll_ctl((__wasi_fd_t)-1, __WASI_EPOLL_CTL_ADD, rfd, &ev) == __WASI_ERRNO_BADF);

    assert(__wasi_epoll_ctl(rfd, __WASI_EPOLL_CTL_ADD, rfd, &ev) == __WASI_ERRNO_INVAL);

    assert(__wasi_epoll_ctl(epfd, (__wasi_epoll_ctl_t)12345, rfd, &ev) == __WASI_ERRNO_INVAL);

    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_ADD, rfd, NULL) == __WASI_ERRNO_INVAL);

    ev.events = 0;
    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_ADD, rfd, &ev) == __WASI_ERRNO_INVAL);

    ev.events = (__wasi_epoll_type_t)0x80000000u;
    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_ADD, rfd, &ev) == __WASI_ERRNO_INVAL);

    ev.events = __WASI_EPOLL_TYPE_EPOLLIN;
    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_ADD, (__wasi_fd_t)-1, &ev) == __WASI_ERRNO_BADF);

    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_ADD, epfd, &ev) == __WASI_ERRNO_INVAL);

    assert(__wasi_fd_close(rfd) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(wfd) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(epfd) == __WASI_ERRNO_SUCCESS);
}

static void test_delete_missing_and_null_event(void)
{
    printf("Test 4: delete missing + NULL event\n");

    __wasi_fd_t epfd = create_epoll_fd();
    __wasi_fd_t rfd = 0;
    __wasi_fd_t wfd = 0;
    create_pipe(&rfd, &wfd);

    __wasi_epoll_event_t ev = make_event(__WASI_EPOLL_TYPE_EPOLLIN, rfd);

    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_ADD, rfd, &ev) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_DEL, rfd, NULL) == __WASI_ERRNO_SUCCESS);

    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_DEL, rfd, &ev) == __WASI_ERRNO_NOENT);

    assert(__wasi_fd_close(rfd) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(wfd) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(epfd) == __WASI_ERRNO_SUCCESS);
}

static void test_unaligned_and_invalid_pointer(void)
{
    printf("Test 5: unaligned and invalid event pointer\n");

    __wasi_fd_t epfd = create_epoll_fd();
    __wasi_fd_t rfd = 0;
    __wasi_fd_t wfd = 0;
    create_pipe(&rfd, &wfd);

    __wasi_epoll_event_t ev = make_event(__WASI_EPOLL_TYPE_EPOLLIN, rfd);

    unsigned char buf[sizeof(__wasi_epoll_event_t) + 1];
    memset(buf, 0, sizeof(buf));
    memcpy(buf + 1, &ev, sizeof(ev));
    __wasi_epoll_event_t *unaligned = (__wasi_epoll_event_t *)(buf + 1);

    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_ADD, rfd, unaligned) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_DEL, rfd, unaligned) == __WASI_ERRNO_SUCCESS);

    __wasi_epoll_event_t *bad_ptr = (__wasi_epoll_event_t *)(uintptr_t)0xFFFFFFFFu;
    assert(__wasi_epoll_ctl(epfd, __WASI_EPOLL_CTL_ADD, rfd, bad_ptr) == __WASI_ERRNO_MEMVIOLATION);

    assert(__wasi_fd_close(rfd) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(wfd) == __WASI_ERRNO_SUCCESS);
    assert(__wasi_fd_close(epfd) == __WASI_ERRNO_SUCCESS);
}

int main(void)
{
    printf("WASIX epoll_ctl integration tests\n");
    test_basic_add_del_duplicate();
    test_mod_and_wait();
    test_invalid_args();
    test_delete_missing_and_null_event();
    test_unaligned_and_invalid_pointer();
    printf("All tests passed!\n");
    return 0;
}
