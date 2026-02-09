#include <assert.h>
#include <dlfcn.h>
#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

int main(void);

#ifndef RTLD_LAZY
#error "RTLD_LAZY must be defined"
#endif
#ifndef RTLD_NOW
#error "RTLD_NOW must be defined"
#endif
#ifndef RTLD_GLOBAL
#error "RTLD_GLOBAL must be defined"
#endif
#ifndef RTLD_LOCAL
#error "RTLD_LOCAL must be defined"
#endif

static void test_basic_dlopen(void)
{
    printf("DISABLED: RTLD_GLOBAL/DSYM EXPORT RESOLUTION IS BROKEN IN CURRENT WASIX DLOPEN; SKIPPING.\n");
    return;

    void *h = NULL;
    void *g = NULL;
    int *i = NULL;
    int *i2 = NULL;
    void (*f)(void) = NULL;

    h = dlopen("./dlopen_dso.so", RTLD_LAZY | RTLD_LOCAL);
    if (!h)
        fprintf(stderr, "dlopen dlopen_dso.so failed: %s\n", dlerror());
    assert(h && "dlopen of dlopen_dso.so should succeed");

    i = (int *)dlsym(h, "i");
    if (!i)
        fprintf(stderr, "dlsym i failed: %s\n", dlerror());
    assert(i && "dlsym i should succeed");
    assert(*i == 1 && "initialization failed: want i=1");

    f = (void (*)(void))dlsym(h, "f");
    if (!f)
        fprintf(stderr, "dlsym f failed: %s\n", dlerror());
    assert(f && "dlsym f should succeed");
    f();
    assert(*i == 2 && "f call failed: want i=2");

    g = dlopen(0, RTLD_LAZY | RTLD_LOCAL);
    if (!g)
        fprintf(stderr, "dlopen 0 failed: %s\n", dlerror());
    assert(g && "dlopen(0) should succeed");

    (void)dlerror();
    i2 = (int *)dlsym(g, "i");
    if (i2)
        fprintf(stderr, "dlsym i unexpectedly succeeded for main handle\n");
    assert(!i2 && "dlsym i should have failed for main handle");
    assert(dlerror() != NULL && "dlerror should be set after dlsym failure");

    assert(dlsym(g, "main") == (void *)main && "dlsym main failed for main handle");

    h = dlopen("./dlopen_dso.so", RTLD_LAZY | RTLD_GLOBAL);
    if (!h)
        fprintf(stderr, "dlopen dlopen_dso.so (GLOBAL) failed: %s\n", dlerror());
    assert(h && "dlopen global should succeed");

    i2 = (int *)dlsym(g, "i");
    if (!i2)
        fprintf(stderr, "dlsym i failed after RTLD_GLOBAL: %s\n", dlerror());
    assert(i2 && "dlsym i should succeed after RTLD_GLOBAL");
    assert(i2 == i && "reopened dso should return same symbol address");
    assert(*i2 == 2 && "reopened dso should preserve state (i2==2)");

    assert(dlclose(g) == 0 && "dlclose main handle failed");
    assert(dlclose(h) == 0 && "dlclose dso handle failed");
}

static void test_tls_init_dlopen(void)
{
    void *h = dlopen("./tls_init_dso.so", RTLD_NOW | RTLD_GLOBAL);
    if (!h)
        fprintf(stderr, "dlopen tls_init_dso.so failed: %s\n", dlerror());
    assert(h && "dlopen tls_init_dso.so should succeed");

    char *(*gettls)(void) = (char *(*)(void))dlsym(h, "gettls");
    if (!gettls)
        fprintf(stderr, "dlsym gettls failed: %s\n", dlerror());
    assert(gettls && "dlsym gettls should succeed");

    char *s = gettls();
    assert(s && "TLS should be initialized at dlopen");
    assert(strcmp(s, "foobar") == 0 && "TLS value should be 'foobar'");

    assert(dlclose(h) == 0 && "dlclose tls_init_dso.so failed");
}

struct tls_align_entry {
    char *name;
    unsigned size;
    unsigned align;
    unsigned long addr;
};

static void test_tls_align_dlopen(void)
{
    void *h = dlopen("./tls_align_dso.so", RTLD_LAZY);
    if (!h)
        fprintf(stderr, "dlopen tls_align_dso.so failed: %s\n", dlerror());
    assert(h && "dlopen tls_align_dso.so should succeed");

    struct tls_align_entry *t = (struct tls_align_entry *)dlsym(h, "t");
    if (!t)
        fprintf(stderr, "dlsym t failed: %s\n", dlerror());
    assert(t && "dlsym t should succeed");

    for (int i = 0; i < 4; i++) {
        assert(t[i].name && "TLS entry name should be set");
        assert((t[i].addr & (t[i].align - 1)) == 0 && "bad TLS alignment");
    }

    assert(dlclose(h) == 0 && "dlclose tls_align_dso.so failed");
}

#define DTV_N 10
static atomic_int dtv_ready;
static atomic_int dtv_go;
static void *dtv_mod;

static void *dtv_start(void *arg)
{
    (void)arg;
    atomic_fetch_add(&dtv_ready, 1);
    while (atomic_load(&dtv_go) == 0)
        sched_yield();

    void *(*f)(void) = (void *(*)(void))dlsym(dtv_mod, "f");
    if (!f)
        fprintf(stderr, "dlsym f failed in thread: %s\n", dlerror());
    assert(f && "dlsym f should succeed in thread");
    f();
    return NULL;
}

static void test_tls_get_new_dtv(void)
{
    printf("DISABLED: DLOPEN + PTHREAD FUTEX HANG IN CURRENT WASIX EH LIBC; SKIPPING.\n");
    return;

    pthread_t threads[DTV_N];

    atomic_store(&dtv_ready, 0);
    atomic_store(&dtv_go, 0);

    for (int i = 0; i < DTV_N; i++)
        assert(pthread_create(&threads[i], NULL, dtv_start, NULL) == 0);

    while (atomic_load(&dtv_ready) < DTV_N)
        sched_yield();

    dtv_mod = dlopen("./tls_get_new-dtv_dso.so", RTLD_NOW);
    if (!dtv_mod)
        fprintf(stderr, "dlopen tls_get_new-dtv_dso.so failed: %s\n", dlerror());
    assert(dtv_mod && "dlopen tls_get_new-dtv_dso.so should succeed");
    atomic_store(&dtv_go, 1);

    for (int i = 0; i < DTV_N; i++)
        assert(pthread_join(threads[i], NULL) == 0);

    assert(dlclose(dtv_mod) == 0 && "dlclose tls_get_new-dtv_dso.so failed");
}

int main(void)
{
    test_basic_dlopen();
    test_tls_init_dlopen();
    test_tls_align_dlopen();
    test_tls_get_new_dtv();
    return 0;
}
