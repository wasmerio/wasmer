#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <wasi/api_wasix.h>

static void expect_errno(const char *name, __wasi_errno_t expected)
{
    char args[512];
    int args_len = snprintf(args, sizeof(args), "%s", name);
    assert(args_len > 0 && args_len < (int)sizeof(args));

    __wasi_errno_t err = __wasi_proc_exec3(name,
                                           args,
                                           "",
                                           __WASI_BOOL_FALSE,
                                           "");
    if (err != expected) {
        printf("Unexpected errno for %s: got %d expected %d\n", name, err, expected);
    }
    assert(err == expected);
}

int main(void)
{
    int fd;

    fd = open("noexec_file", O_CREAT | O_TRUNC | O_WRONLY, 0755);
    assert(fd >= 0);
    assert(close(fd) == 0);

    fd = open("noaccess_file", O_CREAT | O_TRUNC | O_WRONLY, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);

    fd = open("notdir", O_CREAT | O_TRUNC | O_WRONLY, 0644);
    assert(fd >= 0);
    assert(close(fd) == 0);

    expect_errno("no_such_file.wasm", __WASI_ERRNO_NOENT);
    expect_errno("notdir/child.wasm", __WASI_ERRNO_NOTDIR);
    expect_errno("noexec_file", __WASI_ERRNO_NOEXEC);
    // NOTE: WASIX does not model exec permission bits; invalid modules return NOEXEC.
    expect_errno("noaccess_file", __WASI_ERRNO_NOEXEC);

    char long_name[300];
    memset(long_name, 'a', sizeof(long_name));
    long_name[sizeof(long_name) - 1] = '\0';
    expect_errno(long_name, __WASI_ERRNO_NAMETOOLONG);

    return 0;
}
