#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

static __wasi_addr_cidr_t make_ipv4_cidr(uint8_t a, uint8_t b, uint8_t c, uint8_t d, uint8_t prefix)
{
    __wasi_addr_cidr_t cidr;
    memset(&cidr, 0, sizeof(cidr));
    cidr.tag = __WASI_ADDRESS_FAMILY_INET4;
    cidr.u.inet4.addr.n0 = a;
    cidr.u.inet4.addr.n1 = b;
    cidr.u.inet4.addr.h0 = c;
    cidr.u.inet4.addr.h1 = d;
    cidr.u.inet4.prefix = prefix;
    return cidr;
}

static __wasi_addr_cidr_t make_invalid_cidr(void)
{
    __wasi_addr_cidr_t cidr;
    memset(&cidr, 0, sizeof(cidr));
    cidr.tag = __WASI_ADDRESS_FAMILY_UNIX;
    return cidr;
}

static void test_invalid_pointer(void)
{
    printf("Test 1: invalid cidr pointer\n");
    __wasi_addr_cidr_t *bad_ptr = (__wasi_addr_cidr_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_port_addr_add(bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

static void test_invalid_tag(void)
{
    printf("Test 2: invalid address family returns INVAL\n");
    __wasi_addr_cidr_t cidr = make_invalid_cidr();
    __wasi_errno_t err = __wasi_port_addr_add(&cidr);
    assert(err == __WASI_ERRNO_INVAL);
}

static void test_host_net_not_supported(void)
{
    printf("Test 3: host networking returns NOTSUP\n");
    __wasi_addr_cidr_t cidr = make_ipv4_cidr(127, 0, 0, 1, 32);
    __wasi_errno_t err = __wasi_port_addr_add(&cidr);
    // NOTE: host networking backend does not allow mutating interface addresses.
    assert(err == __WASI_ERRNO_NOTSUP);
}

int main(void)
{
    printf("WASIX port_addr_add integration tests\n");
    test_invalid_pointer();
    test_invalid_tag();
    test_host_net_not_supported();
    printf("All tests passed!\n");
    return 0;
}
