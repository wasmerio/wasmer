#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

static int port_route_list_supported = 0;
static __wasi_size_t last_route_count = 0;

static void assert_cidr_valid(const __wasi_addr_cidr_t *cidr)
{
    assert(cidr->tag == __WASI_ADDRESS_FAMILY_INET4 ||
           cidr->tag == __WASI_ADDRESS_FAMILY_INET6);

    if (cidr->tag == __WASI_ADDRESS_FAMILY_INET4) {
        assert(cidr->u.inet4.prefix <= 32);
    } else {
        assert(cidr->u.inet6.prefix <= 128);
    }
}

static void assert_addr_valid(const __wasi_addr_t *addr)
{
    assert(addr->tag == __WASI_ADDRESS_FAMILY_INET4 ||
           addr->tag == __WASI_ADDRESS_FAMILY_INET6);
}

static void assert_option_timestamp_valid(const __wasi_option_timestamp_t *ts)
{
    assert(ts->tag == __WASI_OPTION_NONE || ts->tag == __WASI_OPTION_SOME);
    if (ts->tag == __WASI_OPTION_NONE) {
        assert(ts->u.none == 0);
    }
}

static void test_buffer_sizing(void)
{
    printf("Test 1: buffer sizing and overflow\n");

    __wasi_size_t max_routes = 1;
    __wasi_route_t routes[1];
    memset(routes, 0xAA, sizeof(routes));

    __wasi_errno_t err = __wasi_port_route_list(routes, &max_routes);
    assert(err != __WASI_ERRNO_NOTSUP);

    port_route_list_supported = 1;
    last_route_count = max_routes;

    if (err == __WASI_ERRNO_OVERFLOW) {
        assert(last_route_count > 1);
        const unsigned char *bytes = (const unsigned char *)routes;
        for (size_t i = 0; i < sizeof(routes); i++) {
            assert(bytes[i] == 0xAA);
        }
    } else {
        assert(err == __WASI_ERRNO_SUCCESS);
        assert(last_route_count <= 1);
        if (last_route_count == 1) {
            assert_cidr_valid(&routes[0].cidr);
            assert_addr_valid(&routes[0].via_router);
            assert_option_timestamp_valid(&routes[0].preferred_until);
            assert_option_timestamp_valid(&routes[0].expires_at);
        }
    }
}

static void test_full_read(void)
{
    printf("Test 2: full read\n");

    assert(port_route_list_supported);
    assert(last_route_count > 0);

    __wasi_route_t *routes = (__wasi_route_t *)calloc(
        last_route_count, sizeof(__wasi_route_t));
    assert(routes != NULL);

    __wasi_size_t count = last_route_count;
    __wasi_errno_t err = __wasi_port_route_list(routes, &count);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(count == last_route_count);

    for (__wasi_size_t i = 0; i < count; i++) {
        assert_cidr_valid(&routes[i].cidr);
        assert_addr_valid(&routes[i].via_router);
        assert_option_timestamp_valid(&routes[i].preferred_until);
        assert_option_timestamp_valid(&routes[i].expires_at);
    }

    free(routes);
}

static void test_invalid_nroutes_pointer(void)
{
    printf("Test 3: invalid nroutes pointer\n");

    __wasi_size_t *bad_ptr = (__wasi_size_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_port_route_list(NULL, bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

static void test_invalid_routes_pointer(void)
{
    printf("Test 4: invalid routes pointer\n");

    assert(port_route_list_supported);
    assert(last_route_count > 0);

    __wasi_route_t *bad_ptr = (__wasi_route_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_size_t count = last_route_count;
    __wasi_errno_t err = __wasi_port_route_list(bad_ptr, &count);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    printf("WASIX port_route_list integration tests\n");
    test_buffer_sizing();
    test_full_read();
    test_invalid_nroutes_pointer();
    test_invalid_routes_pointer();
    printf("All tests passed!\n");
    return 0;
}
