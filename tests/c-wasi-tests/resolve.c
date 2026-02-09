#include <assert.h>
#include <arpa/inet.h>
#include <netdb.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <wasi/api_wasix.h>

static void require_addrinfo_v4(struct addrinfo *res, uint16_t port, uint32_t addr_be)
{
    struct addrinfo *p = res;
    int found = 0;

    for (; p; p = p->ai_next) {
        if (p->ai_family != AF_INET || p->ai_addr == NULL) {
            continue;
        }
        struct sockaddr_in *sin = (struct sockaddr_in *)p->ai_addr;
        if (sin->sin_port == htons(port) && sin->sin_addr.s_addr == addr_be) {
            found = 1;
            break;
        }
    }

    assert(found && "expected IPv4 addrinfo entry not found");
}

static void test_numeric_ipv4_basic(void)
{
    printf("Test 1: numeric IPv4 host + numeric service\n");
    struct addrinfo hints;
    struct addrinfo *res = NULL;

    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;

    int ret = getaddrinfo("127.0.0.1", "80", &hints, &res);
    assert(ret == 0);
    assert(res != NULL);

    require_addrinfo_v4(res, 80, htonl(INADDR_LOOPBACK));
    freeaddrinfo(res);
}

static void test_passive_null_host(void)
{
    printf("Test 2: AI_PASSIVE with NULL host returns INADDR_ANY\n");
    struct addrinfo hints;
    struct addrinfo *res = NULL;

    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_flags = AI_PASSIVE;

    int ret = getaddrinfo(NULL, "9462", &hints, &res);
    assert(ret == 0);
    assert(res != NULL);

    require_addrinfo_v4(res, 9462, htonl(INADDR_ANY));
    freeaddrinfo(res);
}

static void test_null_host_loopback(void)
{
    printf("Test 3: NULL host without AI_PASSIVE returns loopback\n");
    struct addrinfo hints;
    struct addrinfo *res = NULL;

    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;

    int ret = getaddrinfo(NULL, "9462", &hints, &res);
    assert(ret == 0);
    assert(res != NULL);

    require_addrinfo_v4(res, 9462, htonl(INADDR_LOOPBACK));
    freeaddrinfo(res);
}

static void test_ai_numerichost_non_numeric(void)
{
    printf("Test 4: AI_NUMERICHOST with non-numeric host fails\n");
    struct addrinfo hints;
    struct addrinfo *res = NULL;

    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_INET;
    hints.ai_flags = AI_NUMERICHOST;

    int ret = getaddrinfo("not-a-number", "80", &hints, &res);
    assert(ret == EAI_NONAME);
    assert(res == NULL);
}

static void test_ai_numericserv_non_numeric(void)
{
    printf("Test 5: AI_NUMERICSERV with non-numeric service fails\n");
    struct addrinfo hints;
    struct addrinfo *res = NULL;

    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_INET;
    hints.ai_flags = AI_NUMERICSERV;

    int ret = getaddrinfo("127.0.0.1", "echo", &hints, &res);
    assert(ret == EAI_NONAME);
    assert(res == NULL);
}

static void test_socktype_protocol_mismatch(void)
{
    printf("Test 6: SOCK_STREAM with IPPROTO_UDP fails\n");
    struct addrinfo hints;
    struct addrinfo *res = NULL;

    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_protocol = IPPROTO_UDP;

    int ret = getaddrinfo("127.0.0.1", "80", &hints, &res);
    assert(ret != 0);
    assert(res == NULL);
}

static void test_wasi_resolve_ipv4(void)
{
    printf("Test 7: __wasi_resolve returns IPv4 loopback\n");
    __wasi_addr_ip_t addrs[1];
    __wasi_size_t naddrs = 1;

    __wasi_errno_t err = __wasi_resolve("127.0.0.1", 0, addrs, 1, &naddrs);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(naddrs == 1);
    assert(addrs[0].tag == __WASI_ADDRESS_FAMILY_IP_INET4);
    assert(addrs[0].u.inet4.n0 == 127);
    assert(addrs[0].u.inet4.n1 == 0);
    assert(addrs[0].u.inet4.h0 == 0);
    assert(addrs[0].u.inet4.h1 == 1);
}

int main(void)
{
    printf("WASIX resolve/getaddrinfo integration tests\n");
    test_numeric_ipv4_basic();
    test_passive_null_host();
    test_null_host_loopback();
    test_ai_numerichost_non_numeric();
    test_ai_numericserv_non_numeric();
    test_socktype_protocol_mismatch();
    test_wasi_resolve_ipv4();
    printf("All tests passed!\n");
    return 0;
}
