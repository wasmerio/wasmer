#include <stdio.h>
#include <stdlib.h>

extern int data_export;
extern int func_export();

void __attribute__((constructor)) main_ctor()
{
    printf("Main loaded\n");
}

void __attribute__((destructor)) main_dtor()
{
    printf("Main unloaded\n");
}

int main()
{
    printf("Main started\n");

    printf("data_export = %d\n", data_export);

    printf("calling func_export directly\n");
    printf("result: %d\n", func_export());

    printf("calling func_export via pointer\n");
    int (*func_export_ptr)() = &func_export;
    (*func_export_ptr)();

    printf("done\n");

    return 0;
}