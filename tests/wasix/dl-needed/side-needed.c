#include <stdio.h>

// This generates a GOT.func entry that gets resolved between
// loading this module and side.c, which caused an error in the past.
extern int main_exported();
int (*main_exported_ptr)() = main_exported;

int side_needed_func(int x)
{
    // We need a function pointer internal to the module to give the module
    // its own internal table space; this will trigger the GOT.func issue
    // mentioned above.
    int (*side_needed_func_ptr)(int) = side_needed_func;
    if (side_needed_func_ptr != side_needed_func)
    {
        printf("side_needed_func pointer mismatch\n");
        return -1;
    }

    if (main_exported_ptr() != 85)
    {
        printf("main_exported returned unexpected value\n");
        return -1;
    }

    return x + 4;
}