#include <assert.h>
#include <wasi/api.h>

int main(void)
{
    __wasi_errno_t err = __wasi_proc_snapshot();
    assert(err == __WASI_ERRNO_SUCCESS);

    err = __wasi_proc_snapshot();
    assert(err == __WASI_ERRNO_SUCCESS);

    return 0;
}
