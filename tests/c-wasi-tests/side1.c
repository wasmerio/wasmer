#include <stdio.h>

int data_export = 42;

extern int data_export2;
extern int func_export2();

void __attribute__((constructor)) side1_ctor()
{
    printf("Side 1 loaded\n");
}

void __attribute__((destructor)) side1_dtor()
{
    printf("Side 1 unloaded\n");
}

static void local_function(int *i)
{
    printf("local_function called with %d\n", *i);
}

int func_export()
{
    printf("func_export started\n");
    int x = 123;
    local_function(&x);

    printf("calling func_export2\n");
    printf("result: %d\n", func_export2());

    printf("calling func_export2 via pointer\n");
    int (*func_export2_ptr)() = &func_export2;
    (*func_export2_ptr)();

    printf("data_export2: %d\n", data_export2);
    return 234;
}