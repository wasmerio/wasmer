#include <assert.h>
#include <wasi/api_wasi.h>

int main(void)
{
    const __wasi_exitcode_t code = 1;
    assert(code == 1);
    __wasi_proc_exit(code);
    assert(0 && "proc_exit returned");
    return 0;
}
