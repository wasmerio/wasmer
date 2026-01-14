#include <stdio.h>
#include <stdlib.h>
#include <dlfcn.h>
#include <stdint.h>

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

    // Test dl_invalid_handle: invalid handle 0 (NULL) - should fail
    printf("testing invalid handle 0 (NULL)...\n");
    if (dlclose((void *)0) == 0)
    {
        fprintf(stderr, "expected dlclose to fail for NULL handle\n");
        return 1;
    }
    char *error = dlerror();
    if (!error || *error == '\0')
    {
        fprintf(stderr, "dlerror should not be empty after NULL dlclose\n");
        return 1;
    }

    // Test dl_invalid_handle: invalid handle 0xffffff
    printf("testing invalid handle 0xffffff...\n");
    if (dlclose((void *)(uintptr_t)0xffffff) == 0)
    {
        fprintf(stderr, "expected dlclose to fail for bad handle 0xffffff\n");
        return 1;
    }
    error = dlerror();
    if (!error || *error == '\0')
    {
        fprintf(stderr, "dlerror should not be empty after bad dlclose\n");
        return 1;
    }

    // Test dl_invalid_handle: invalid handle with max u32 value
    printf("testing invalid handle 0xFFFFFFFF...\n");
    if (dlclose((void *)(uintptr_t)0xFFFFFFFF) == 0)
    {
        fprintf(stderr, "expected dlclose to fail for max u32 handle\n");
        return 1;
    }
    error = dlerror();
    if (!error || *error == '\0')
    {
        fprintf(stderr, "dlerror should not be empty after max u32 dlclose\n");
        return 1;
    }

    // Test dl_invalid_handle: some small sequential invalid handles (1-5)
    printf("testing small sequential invalid handles...\n");
    for (int i = 1; i <= 5; i++)
    {
        if (dlclose((void *)(uintptr_t)i) == 0)
        {
            fprintf(stderr, "expected dlclose to fail for small handle %d\n", i);
            return 1;
        }
        error = dlerror();
        if (!error || *error == '\0')
        {
            fprintf(stderr, "dlerror should not be empty after dlclose(%d)\n", i);
            return 1;
        }
    }

    // Test dl_invalid_handle: power-of-2 invalid handles
    printf("testing power-of-2 invalid handles...\n");
    unsigned int powers[] = {2, 4, 8, 16, 32, 64, 128, 256, 512, 1024};
    for (int i = 0; i < sizeof(powers) / sizeof(powers[0]); i++)
    {
        if (dlclose((void *)(uintptr_t)powers[i]) == 0)
        {
            fprintf(stderr, "expected dlclose to fail for power-of-2 handle %u\n", powers[i]);
            return 1;
        }
        error = dlerror();
        if (!error || *error == '\0')
        {
            fprintf(stderr, "dlerror should not be empty after dlclose(%u)\n", powers[i]);
            return 1;
        }
    }

    printf("done!\n");

    return 0;
}
