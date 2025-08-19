#include <stdio.h>

extern int tls_base[4];

int side_static_int = 1;
_Thread_local int *side_tls_ptr = &side_static_int;

_Thread_local extern int tls_var;

int side_func(int in_thread, int value)
{
    if (side_tls_ptr != &side_static_int)
    {
        fprintf(stderr, "TLS pointer does not point to static_int\n");
        return 1;
    }

    tls_base[in_thread ? 1 : 3] = (int)__builtin_wasm_tls_base();
    tls_var = value;
    return 0;
}