#include <assert.h>
#include <wasi/api_wasi.h>

int main(void)
{
    volatile double a = 1.0;
    volatile double b = 2.0;
    volatile double c = 3.0;
    volatile double d = 4.0;
    volatile double sum = a + b + c + d;
    assert(sum > 0.0);
    __wasi_proc_exit((__wasi_exitcode_t)0);
    assert(0 && "proc_exit returned");
    return 0;
}
