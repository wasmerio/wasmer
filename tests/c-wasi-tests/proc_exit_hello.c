#include <assert.h>
#include <unistd.h>
#include <wasi/api_wasi.h>

int main(void)
{
    const char msg[] = "Hello, world!\n";
    ssize_t written = write(STDOUT_FILENO, msg, sizeof(msg) - 1);
    assert(written == (ssize_t)(sizeof(msg) - 1));
    __wasi_proc_exit((__wasi_exitcode_t)1);
    assert(0 && "proc_exit returned");
    return 0;
}
