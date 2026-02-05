#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

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

static __wasi_addr_t make_invalid_addr(void)
{
    __wasi_addr_t addr;
    memset(&addr, 0, sizeof(addr));
    addr.tag = __WASI_ADDRESS_FAMILY_UNIX;
    return addr;
}

static __wasi_option_timestamp_t make_none_ts(void)
{
    __wasi_option_timestamp_t opt;
    memset(&opt, 0, sizeof(opt));
    opt.tag = 0; // None
    return opt;
}

static __wasi_option_timestamp_t make_invalid_ts(void)
{
    __wasi_option_timestamp_t opt;
    memset(&opt, 0, sizeof(opt));
    opt.tag = 2; // invalid
    return opt;
}

static void test_invalid_cidr_ptr(void)
{
    printf("Test 1: invalid CIDR pointer\n");
    __wasi_addr_t via = make_ipv4(127, 0, 0, 1);
    __wasi_option_timestamp_t none = make_none_ts();
    __wasi_addr_cidr_t *bad = (__wasi_addr_cidr_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_port_route_add(bad, &via, &none, &none);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

static void test_invalid_router_ptr(void)
{
    printf("Test 2: invalid via_router pointer\n");
    __wasi_addr_cidr_t cidr = make_ipv4_cidr(10, 0, 0, 0, 24);
    __wasi_option_timestamp_t none = make_none_ts();
    __wasi_addr_t *bad = (__wasi_addr_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_port_route_add(&cidr, bad, &none, &none);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

static void test_invalid_cidr_tag(void)
{
    printf("Test 3: invalid CIDR family returns INVAL\n");
    __wasi_addr_cidr_t cidr = make_invalid_cidr();
    __wasi_addr_t via = make_ipv4(127, 0, 0, 1);
    __wasi_option_timestamp_t none = make_none_ts();
    __wasi_errno_t err = __wasi_port_route_add(&cidr, &via, &none, &none);
    assert(err == __WASI_ERRNO_INVAL);
}

static void test_invalid_router_tag(void)
{
    printf("Test 4: invalid router family returns INVAL\n");
    __wasi_addr_cidr_t cidr = make_ipv4_cidr(10, 0, 0, 0, 24);
    __wasi_addr_t via = make_invalid_addr();
    __wasi_option_timestamp_t none = make_none_ts();
    __wasi_errno_t err = __wasi_port_route_add(&cidr, &via, &none, &none);
    assert(err == __WASI_ERRNO_INVAL);
}

static void test_host_net_not_supported(void)
{
    printf("Test 6: host networking returns NOTSUP\n");
    __wasi_addr_cidr_t cidr = make_ipv4_cidr(10, 0, 0, 0, 24);
    __wasi_addr_t via = make_ipv4(127, 0, 0, 1);
    __wasi_option_timestamp_t none = make_none_ts();
    __wasi_errno_t err = __wasi_port_route_add(&cidr, &via, &none, &none);
    assert(err == __WASI_ERRNO_NOTSUP);
}

int main(void)
{
    printf("WASIX port_route_add integration tests\n");
    test_invalid_cidr_ptr();
    test_invalid_router_ptr();
    test_invalid_cidr_tag();
    test_invalid_router_tag();
    test_host_net_not_supported();
    printf("All tests passed!\n");
    return 0;
}
