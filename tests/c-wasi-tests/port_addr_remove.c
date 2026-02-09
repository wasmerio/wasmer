#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

static __wasi_addr_t make_ipv4(uint8_t a, uint8_t b, uint8_t c, uint8_t d)
{
    __wasi_addr_t addr;
    memset(&addr, 0, sizeof(addr));
    addr.tag = __WASI_ADDRESS_FAMILY_INET4;
    addr.u.inet4.n0 = a;
    addr.u.inet4.n1 = b;
    addr.u.inet4.h0 = c;
    addr.u.inet4.h1 = d;
    return addr;
}

static void test_invalid_pointer(void)
{
    printf("Test 1: invalid address pointer\n");
    __wasi_addr_t *bad_ptr = (__wasi_addr_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_port_addr_remove(bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

static void test_not_supported_host_net(void)
{
    printf("Test 2: host networking returns NOTSUP\n");
    __wasi_addr_t addr = make_ipv4(127, 0, 0, 1);
    __wasi_errno_t err = __wasi_port_addr_remove(&addr);
    // NOTE: host networking backend does not allow mutating interface addresses.
    assert(err == __WASI_ERRNO_NOTSUP);
}

int main(void)
{
    printf("WASIX port_addr_remove integration tests\n");
    test_invalid_pointer();
    test_not_supported_host_net();
    printf("All tests passed!\n");
    return 0;
}
