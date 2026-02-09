#include <assert.h>
#include <wasi/api_wasi.h>

int main(void)
{
    const __wasi_exitcode_t code = 2;
    assert(code == 2);
    __wasi_proc_exit(code);
    assert(0 && "proc_exit returned");
    return 0;
}
