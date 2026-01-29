#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <wasi/api.h>
#include <wasi/api_wasix.h>

static int port_addr_list_supported = 0;
static __wasi_size_t last_addr_count = 0;

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

static void test_buffer_sizing(void)
{
    printf("Test 1: buffer sizing and overflow\n");

    __wasi_size_t max_addrs = 1;
    __wasi_addr_cidr_t addrs[1];
    memset(addrs, 0xAA, sizeof(addrs));

    __wasi_errno_t err = __wasi_port_addr_list(addrs, &max_addrs);
    assert(err != __WASI_ERRNO_NOTSUP);

    port_addr_list_supported = 1;
    last_addr_count = max_addrs;

    if (err == __WASI_ERRNO_OVERFLOW) {
        assert(last_addr_count > 1);
        const unsigned char *bytes = (const unsigned char *)addrs;
        for (size_t i = 0; i < sizeof(addrs); i++) {
            assert(bytes[i] == 0xAA);
        }
    } else {
        assert(err == __WASI_ERRNO_SUCCESS);
        assert(last_addr_count <= 1);
        if (last_addr_count == 1) {
            assert_cidr_valid(&addrs[0]);
        }
    }
}

static void test_full_read(void)
{
    printf("Test 2: full read\n");

    assert(port_addr_list_supported);
    assert(last_addr_count > 0);

    __wasi_addr_cidr_t *addrs = (__wasi_addr_cidr_t *)calloc(
        last_addr_count, sizeof(__wasi_addr_cidr_t));
    assert(addrs != NULL);

    __wasi_size_t count = last_addr_count;
    __wasi_errno_t err = __wasi_port_addr_list(addrs, &count);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(count == last_addr_count);

    for (__wasi_size_t i = 0; i < count; i++) {
        assert_cidr_valid(&addrs[i]);
    }

    free(addrs);
}

static void test_invalid_naddrs_pointer(void)
{
    printf("Test 3: invalid naddrs pointer\n");

    __wasi_size_t *bad_ptr = (__wasi_size_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_errno_t err = __wasi_port_addr_list(NULL, bad_ptr);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

static void test_invalid_addrs_pointer(void)
{
    printf("Test 4: invalid addrs pointer\n");

    assert(port_addr_list_supported);
    assert(last_addr_count > 0);

    __wasi_addr_cidr_t *bad_ptr =
        (__wasi_addr_cidr_t *)(uintptr_t)0xFFFFFFFFu;
    __wasi_size_t count = last_addr_count;
    __wasi_errno_t err = __wasi_port_addr_list(bad_ptr, &count);
    assert(err == __WASI_ERRNO_MEMVIOLATION);
}

int main(void)
{
    printf("WASIX port_addr_list integration tests\n");
    test_buffer_sizing();
    test_full_read();
    test_invalid_naddrs_pointer();
    test_invalid_addrs_pointer();
    printf("All tests passed!\n");
    return 0;
}
