#include <assert.h>
#include <stdio.h>
#include <string.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

static void test_port_mac_not_supported(void)
{
    // No external tests in repo; verify current WASIX behavior is NOTSUP.
    printf("Test 1: port_mac unsupported\n");

    __wasi_hardware_address_t mac;
    memset(&mac, 0xAA, sizeof(mac));

    __wasi_errno_t ret = __wasi_port_mac(&mac);
    assert(ret == __WASI_ERRNO_NOTSUP);

    const unsigned char *bytes = (const unsigned char *)&mac;
    for (size_t i = 0; i < sizeof(mac); i++) {
        assert(bytes[i] == 0xAA);
    }
}

int main(void)
{
    test_port_mac_not_supported();
    printf("All tests passed!\n");
    return 0;
}
