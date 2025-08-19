#include <stdio.h>
#include <stdbool.h>
#include <dlfcn.h>
#include <pthread.h>

extern int side_func(int in_thread, int value);

bool all_unequal(int *arr, size_t size)
{
    for (size_t i = 0; i < size; i++)
    {
        for (size_t j = i + 1; j < size; j++)
        {
            if (arr[i] == arr[j])
            {
                return false;
            }
        }
    }
    return true;
}

// Common storage where all functions can store data for the final check
// Indices are: main-main, main-side, thread-main, thread-side
extern int tls_base[4];

// TLS variable that should be shared between main and side, but be
// different in the thread
extern _Thread_local int tls_var;

// A simple global integer with a TLS pointer
int static_int = 1;
_Thread_local int *tls_ptr = &static_int;

void *thread_func(void *arg)
{
    tls_base[2] = (int)__builtin_wasm_tls_base();

    if (tls_ptr != &static_int)
    {
        fprintf(stderr, "TLS pointer does not point to static_int\n");
        return (void *)1;
    }

    if (tls_var != 0)
    {
        fprintf(stderr, "TLS variable should initially be 0 in thread, got %d\n", tls_var);
        return (void *)1;
    }
    tls_var = 50;
    if (side_func(1, 100) != 0)
    {
        fprintf(stderr, "side_func failed in thread\n");
        return (void *)1;
    }
    if (tls_var != 100)
    {
        fprintf(stderr, "TLS variable not set correctly in thread's side, expected 100, got %d\n", tls_var);
        return (void *)1;
    }

    return (void *)0;
}

int do_main_tests()
{
    tls_base[0] = (int)__builtin_wasm_tls_base();

    // Test if tls_ptr gets set correctly in __wasm_apply_tls_relocs,
    // repeated in the thread and the side module as well
    if (tls_ptr != &static_int)
    {
        fprintf(stderr, "TLS pointer does not point to static_int\n");
        return 1;
    }

    // Test that the side module gets the correct address for main's TLS vars
    // from the linker, repeated in the thread as well
    if (tls_var != 0)
    {
        fprintf(stderr, "TLS variable should initially be 0 in main, got %d\n", tls_var);
        return 1;
    }
    tls_var = 20;
    if (side_func(0, 40) != 0)
    {
        fprintf(stderr, "side_func failed in main\n");
        return 1;
    }
    if (tls_var != 40)
    {
        fprintf(stderr, "TLS variable not set correctly in main's side, expected 40, got %d\n", tls_var);
        return 1;
    }

    return 0;
}

int main()
{
    pthread_t thread;
    if (pthread_create(&thread, NULL, thread_func, NULL) != 0)
    {
        fprintf(stderr, "Failed to create thread\n");
        return 1;
    }

    if (do_main_tests())
    {
        fprintf(stderr, "Main tests failed\n");
        return 1;
    }

    void *thread_ret;
    if (pthread_join(thread, &thread_ret) != 0)
    {
        fprintf(stderr, "Failed to join thread\n");
        return 1;
    }

    if ((int)thread_ret != 0)
    {
        fprintf(stderr, "Thread function failed\n");
        return 1;
    }

    // Make sure each instance got a different TLS base
    if (!all_unequal(tls_base, 4))
    {
        fprintf(stderr, "TLS bases are not unique\n");
        return 1;
    }

    return 0;
}