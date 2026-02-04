#include <assert.h>
#include <stdio.h>

#include <wasi/api_wasix.h>

static void test_host_net_not_supported(void)
{
    printf("Test 1: host networking returns NOTSUP\n");
    __wasi_errno_t err = __wasi_port_unbridge();
    // NOTE: host networking backend does not allow bridge control.
    assert(err == __WASI_ERRNO_NOTSUP);
}

int main(void)
{
    printf("WASIX port_unbridge integration tests\n");
    test_host_net_not_supported();
    printf("All tests passed!\n");
    return 0;
}
