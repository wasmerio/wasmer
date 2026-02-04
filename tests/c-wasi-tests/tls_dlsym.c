#include <stdint.h>
#include <stdalign.h>

__thread char tls_char = 'a';
__thread int tls_int = 123;
__thread long long tls_ll __attribute__((aligned(16))) = 456;
__thread char tls_buf[7] = "foobar";

struct tls_info {
    const char *name;
    unsigned size;
    unsigned align;
    uintptr_t addr;
};

struct tls_info tls_info[4];

__attribute__((constructor))
static void init_tls_info(void)
{
    tls_info[0].name = "tls_char";
    tls_info[0].size = sizeof(tls_char);
    tls_info[0].align = (unsigned)_Alignof(char);
    tls_info[0].addr = (uintptr_t)&tls_char;

    tls_info[1].name = "tls_int";
    tls_info[1].size = sizeof(tls_int);
    tls_info[1].align = (unsigned)_Alignof(int);
    tls_info[1].addr = (uintptr_t)&tls_int;

    tls_info[2].name = "tls_ll";
    tls_info[2].size = sizeof(tls_ll);
    tls_info[2].align = (unsigned)_Alignof(long long);
    tls_info[2].addr = (uintptr_t)&tls_ll;

    tls_info[3].name = "tls_buf";
    tls_info[3].size = sizeof(tls_buf);
    tls_info[3].align = (unsigned)_Alignof(char);
    tls_info[3].addr = (uintptr_t)&tls_buf;
}

char *gettls(void)
{
    return tls_buf;
}
