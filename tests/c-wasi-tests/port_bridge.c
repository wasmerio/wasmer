#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <wasi/api_wasix.h>

static void test_invalid_security(void)
{
    printf("Test 1: invalid security value\n");
    __wasi_errno_t err = __wasi_port_bridge("net", "token",
                                            (__wasi_stream_security_t)0xff);
    assert(err == __WASI_ERRNO_INVAL);
}

static void test_host_networking_not_supported(void)
{
    printf("Test 2: host networking returns NOTSUP\n");
    __wasi_errno_t err = __wasi_port_bridge("net", "token",
                                            __WASI_STREAM_SECURITY_ANY_ENCRYPTION);
    assert(err == __WASI_ERRNO_NOTSUP);
}

int main(void)
{
    printf("WASIX port_bridge integration tests\n");
    test_invalid_security();
    test_host_networking_not_supported();
    printf("All tests passed!\n");
    return 0;
}
