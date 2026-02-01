#include <assert.h>
#include <stdio.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

int main(void)
{
    printf("WASIX port_route_clear integration tests\n");
    __wasi_errno_t err = __wasi_port_route_clear();
    // NOTE: host networking backend does not allow mutating routing tables.
    assert(err == __WASI_ERRNO_NOTSUP);
    printf("All tests passed!\n");
    return 0;
}
