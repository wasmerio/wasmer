#include <assert.h>
#include <stdio.h>

#include <wasi/api_wasix.h>

int main(void) {
    printf("WASIX port_dhcp_acquire integration tests\n");

    __wasi_errno_t err = __wasi_port_dhcp_acquire();
    // NOTE: host networking backend does not implement DHCP.
    assert(err == __WASI_ERRNO_NOTSUP);

    printf("All tests passed!\n");
    return 0;
}
