#include <assert.h>
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

struct tls_info {
    const char *name;
    unsigned size;
    unsigned align;
    uintptr_t addr;
};

static void clear_dlerror(void)
{
    (void)dlerror();
}

static void expect_dlerror_nonempty(const char *label)
{
    char *err = dlerror();
    assert(err != NULL);
    assert(*err != '\0');
    printf("%s: %s\n", label, err);
}

int main(void)
{
    printf("WASIX dlsym integration tests\n");

    printf("Test 1: dlsym data/function from handle\n");
    void *h_local = dlopen("libside1.so", RTLD_LAZY | RTLD_LOCAL);
    assert(h_local != NULL);

    clear_dlerror();
    int *data_export = (int *)dlsym(h_local, "data_export");
    assert(data_export != NULL);
    assert(*data_export == 42);

    clear_dlerror();
    int (*func_export)(void) = (int (*)(void))dlsym(h_local, "func_export");
    assert(func_export != NULL);
    assert(func_export() == 234);

    clear_dlerror();
    void *local_fn = dlsym(h_local, "local_function");
    assert(local_fn == NULL);
    expect_dlerror_nonempty("local_function not exported");

    printf("Test 2: RTLD_DEFAULT visibility and main symbol\n");
    void *h_main = dlopen(0, RTLD_LAZY | RTLD_LOCAL);
    assert(h_main != NULL);

    clear_dlerror();
    void *main_sym = dlsym(h_main, "main");
    assert(main_sym == (void *)main);

    clear_dlerror();
    void *default_local = dlsym(RTLD_DEFAULT, "data_export");
    assert(default_local == NULL);
    expect_dlerror_nonempty("RTLD_DEFAULT local data_export");

    void *h_global = dlopen("libside1.so", RTLD_LAZY | RTLD_GLOBAL);
    assert(h_global != NULL);

    clear_dlerror();
    void *default_global = dlsym(RTLD_DEFAULT, "data_export");
    assert(default_global != NULL);
    assert(default_global == (void *)data_export);
    assert(*(int *)default_global == 42);

    printf("Test 3: invalid handle\n");
    clear_dlerror();
    void *bad = dlsym((void *)(uintptr_t)0xffffff, "data_export");
    assert(bad == NULL);
    expect_dlerror_nonempty("invalid handle");

    printf("Test 4: same symbol name in different libraries\n");
    void *h_cache1 = dlopen("libcache1.so", RTLD_NOW | RTLD_GLOBAL);
    assert(h_cache1 != NULL);
    void *h_cache2 = dlopen("libcache2.so", RTLD_NOW | RTLD_GLOBAL);
    assert(h_cache2 != NULL);

    clear_dlerror();
    int (*side_func1)(int) = (int (*)(int))dlsym(h_cache1, "side_func");
    assert(side_func1 != NULL);
    int (*side_func2)(int) = (int (*)(int))dlsym(h_cache2, "side_func");
    assert(side_func2 != NULL);
    assert(side_func1 != side_func2);
    assert(side_func1(2) == 44);
    assert(side_func2(2) == 4);

    printf("Test 5: TLS via dlsym\n");
    void *h_tls = dlopen("libtls.so", RTLD_NOW | RTLD_GLOBAL);
    assert(h_tls != NULL);

    clear_dlerror();
    char *(*gettls)(void) = (char *(*)(void))dlsym(h_tls, "gettls");
    assert(gettls != NULL);
    char *tls_value = gettls();
    assert(tls_value != NULL);
    assert(strcmp(tls_value, "foobar") == 0);

    clear_dlerror();
    struct tls_info *info = (struct tls_info *)dlsym(h_tls, "tls_info");
    assert(info != NULL);
    for (int i = 0; i < 4; i++) {
        assert(info[i].name != NULL);
        assert(info[i].align != 0);
        assert((info[i].addr & (info[i].align - 1)) == 0);
    }

    assert(dlclose(h_tls) == 0);
    assert(dlclose(h_cache1) == 0);
    assert(dlclose(h_cache2) == 0);
    assert(dlclose(h_global) == 0);
    assert(dlclose(h_main) == 0);
    assert(dlclose(h_local) == 0);

    printf("All tests passed!\n");
    return 0;
}
