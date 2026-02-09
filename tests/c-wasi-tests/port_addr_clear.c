#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#include <wasi/api_wasix.h>

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

static void read_addrs(__wasi_addr_cidr_t *addrs, __wasi_size_t *count)
{
    __wasi_errno_t err = __wasi_port_addr_list(addrs, count);
    assert(err == __WASI_ERRNO_SUCCESS);
    for (__wasi_size_t i = 0; i < *count; i++) {
        assert_cidr_valid(&addrs[i]);
    }
}

static void test_clear_empty(void)
{
    printf("Test 1: clear on empty list\n");

    __wasi_errno_t err = __wasi_port_addr_clear();
    // NOTE: host networking backend is read-only for interface addresses.
    // Expect NOTSUP rather than mutating host interfaces.
    assert(err == __WASI_ERRNO_NOTSUP);
}

int main(void)
{
    printf("WASIX port_addr_clear integration tests\n");
    test_clear_empty();
    printf("All tests passed!\n");
    return 0;
}
