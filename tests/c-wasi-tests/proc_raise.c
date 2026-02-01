#include <assert.h>
#include <signal.h>
#include <stdio.h>

static volatile sig_atomic_t sig_count = 0;
static volatile sig_atomic_t last_sig = 0;

static void handler(int sig)
{
    sig_count++;
    last_sig = sig;
}

static void test_handler(void)
{
    printf("Test 1: handler runs for SIGUSR1\n");
    sig_count = 0;
    last_sig = 0;

    void (*prev)(int) = signal(SIGUSR1, handler);
    assert(prev != SIG_ERR);

    int rc = raise(SIGUSR1);
    assert(rc == 0);
    assert(sig_count == 1);
    assert(last_sig == SIGUSR1);
}

static void test_ignore(void)
{
    printf("Test 2: SIGUSR1 ignored\n");
    sig_count = 0;
    last_sig = 0;

    void (*prev)(int) = signal(SIGUSR1, SIG_IGN);
    assert(prev != SIG_ERR);

    int rc = raise(SIGUSR1);
    assert(rc == 0);
    assert(sig_count == 0);
    assert(last_sig == 0);
}

static void test_raise_zero(void)
{
    printf("Test 3: raise(0) is a no-op\n");
    sig_count = 0;
    last_sig = 0;

    int rc = raise(0);
    assert(rc == 0);
    assert(sig_count == 0);
    assert(last_sig == 0);
}

int main(void)
{
    test_handler();
    test_ignore();
    test_raise_zero();

    printf("proc_raise tests completed\n");
    return 0;
}
