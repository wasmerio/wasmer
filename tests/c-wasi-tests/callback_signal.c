#include <assert.h>
#include <stdint.h>
#include <stdio.h>

#include <wasi/api_wasi.h>
#include <wasi/api_wasix.h>

static volatile int handler_a_calls = 0;
static volatile int handler_b_calls = 0;
static volatile int last_sig = 0;

__attribute__((used, export_name("test_signal_handler_a")))
void test_signal_handler_a(int sig)
{
    handler_a_calls++;
    last_sig = sig;
}

__attribute__((used, export_name("test_signal_handler_b")))
void test_signal_handler_b(int sig)
{
    handler_b_calls++;
    last_sig = sig;
}

static void test_basic_callback(void)
{
    printf("Test 1: callback_signal registers and dispatches\n");
    handler_a_calls = 0;
    handler_b_calls = 0;
    last_sig = 0;

    __wasi_callback_signal("test_signal_handler_a");
    __wasi_errno_t err = __wasi_proc_raise(__WASI_SIGNAL_USR1);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(handler_a_calls == 1);
    assert(handler_b_calls == 0);
    assert(last_sig == __WASI_SIGNAL_USR1);
}

static void test_replace_callback(void)
{
    printf("Test 2: callback_signal replaces handler\n");
    __wasi_callback_signal("test_signal_handler_b");
    __wasi_errno_t err = __wasi_proc_raise(__WASI_SIGNAL_USR2);
    assert(err == __WASI_ERRNO_SUCCESS);
    assert(handler_a_calls == 1);
    assert(handler_b_calls == 1);
    assert(last_sig == __WASI_SIGNAL_USR2);
}

int main(void)
{
    printf("WASIX callback_signal integration tests\n");
    test_basic_callback();
    test_replace_callback();
    printf("All tests passed!\n");
    return 0;
}
