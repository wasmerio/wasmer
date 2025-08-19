#include <stdio.h>
#include <dlfcn.h>

extern int main_needed_func(int);

int main()
{
    if (main_needed_func(42) != 43)
    {
        fprintf(stderr, "main_needed_func returned unexpected value\n");
        return 1;
    }

    void *handle = dlopen("./libside.so", RTLD_NOW | RTLD_GLOBAL);
    if (!handle)
    {
        fprintf(stderr, "dlopen failed: %s\n", dlerror());
        return 1;
    }

    int (*side_func)(int) = dlsym(handle, "side_func");
    if (!side_func)
    {
        fprintf(stderr, "dlsym failed: %s\n", dlerror());
        dlclose(handle);
        return 1;
    }

    // side_func returns (x + 4) * 2
    int res = side_func(42);
    if (res != 92)
    {
        fprintf(stderr, "side_func returned unexpected value: %d\n", res);
        dlclose(handle);
        return 1;
    }

    if (dlclose(handle) != 0)
    {
        fprintf(stderr, "dlclose failed: %s\n", dlerror());
        return 1;
    }

    // Print something to make sure printf and, by extension, data relocations work.
    // Do *NOT* remote this.
    printf("All tests passed successfully!\n");

    return 0;
}