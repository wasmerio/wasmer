// Core-note fixture: keep a named, offset file mapping and a known memory marker.
#define _GNU_SOURCE
#include <assert.h>
#include <fcntl.h>
#include <signal.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <unistd.h>

static volatile char marker[] = "WASIXV2_CORE_READER_MARKER";

int main(void) {
    struct rlimit limit = {RLIM_INFINITY, RLIM_INFINITY};
    assert(setrlimit(RLIMIT_CORE, &limit) == 0);
    long page = sysconf(_SC_PAGESIZE);
    int fd = open("/work/mapped", O_CREAT | O_RDWR | O_TRUNC, 0600);
    assert(fd >= 0);
    assert(ftruncate(fd, 2 * page) == 0);
    void *mapping = mmap(0, page, PROT_READ, MAP_PRIVATE, fd, page);
    assert(mapping != MAP_FAILED);
    assert(marker[0] == 'W');
    assert(chdir("/work") == 0);
    raise(SIGABRT);
    return 1;
}
