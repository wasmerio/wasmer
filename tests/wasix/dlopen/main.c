#include <stdio.h>
#include <dlfcn.h>

int main()
{
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

    int res = side_func(42);
    if (res != 84)
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

    if (dlclose((void *)0xffffff) == 0)
    {
        fprintf(stderr, "expected dlclose to fail for bad handle\n");
        return 1;
    }

    char *error = dlerror();
    if (!error || *error == '\0')
    {
        fprintf(stderr, "dlerror should not be empty after bad dlclose\n");
        return 1;
    }

    // Print something to make sure printf and, by extension, data relocations work.
    // Do *NOT* remote this.
    printf("  All tests passed successfully!\n");

    return 0;
}
