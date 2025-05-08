#include <stdio.h>
#include <stdlib.h>
#include <dlfcn.h>

void __attribute__((constructor)) main_ctor()
{
    printf("Main loaded\n");
}

void __attribute__((destructor)) main_dtor()
{
    printf("Main unloaded\n");
}

// TODO: the side module should run its destructors when it's unloaded
// via dlclose, but currently, it does so when the main module exits.
// This is a known issue with the current linker implementation.
int main()
{
    printf("loading side module...\n");
    void *handle = dlopen("libside1.so", RTLD_NOW);
    if (!handle)
    {
        fprintf(stderr, "failed to open dl: %s\n", dlerror());
        return 1;
    }

    printf("finding data_export...\n");
    int *data_export = dlsym(handle, "data_export");
    if (!data_export)
    {
        fprintf(stderr, "failed to find data_export symbol: %s\n", dlerror());
        return 1;
    }
    if (*data_export != 42)
    {
        fprintf(stderr, "data_export expected to be 42: %d\n", *data_export);
        return 1;
    }
    printf("data_export = %d\n", *data_export);

    printf("finding func_export...\n");
    int (*func_export)() = dlsym(handle, "func_export");
    if (!func_export)
    {
        fprintf(stderr, "failed to find func_export symbol: %s\n", dlerror());
        return 1;
    }

    printf("calling func_export\n");
    printf("result: %d\n", func_export());

    int (*local_function)(int *) = dlsym(handle, "local_function");
    if (local_function)
    {
        fprintf(stderr, "local_function should not be found since it's private\n");
        return 1;
    }

    printf("closing side\n");
    if (dlclose(handle) != 0)
    {
        fprintf(stderr, "failed to unload library: %s\n", dlerror());
        return 1;
    }

    printf("done!\n");

    return 0;
}
