#include <stdio.h>
#include <dlfcn.h>

int main()
{
    void *handle1 = dlopen("./libside1.so", RTLD_NOW | RTLD_GLOBAL);
    if (!handle1)
    {
        fprintf(stderr, "dlopen failed: %s\n", dlerror());
        return 1;
    }

    int (*side_func1)(int) = dlsym(handle1, "side_func");
    if (!side_func1)
    {
        fprintf(stderr, "dlsym failed: %s\n", dlerror());
        dlclose(handle1);
        return 1;
    }

    // side_func1 returns x + 42
    int res = side_func1(2);
    if (res != 44)
    {
        fprintf(stderr, "side_func returned unexpected value: %d\n", res);
        dlclose(handle1);
        return 1;
    }

    void *handle2 = dlopen("./libside2.so", RTLD_NOW | RTLD_GLOBAL);
    if (!handle2)
    {
        fprintf(stderr, "dlopen failed: %s\n", dlerror());
        return 1;
    }

    int (*side_func2)(int) = dlsym(handle2, "side_func");
    if (!side_func2)
    {
        fprintf(stderr, "dlsym failed: %s\n", dlerror());
        dlclose(handle1);
        dlclose(handle2);
        return 1;
    }

    if (side_func1 == side_func2)
    {
        fprintf(stderr, "side_func1 and side_func2 should be different\n");
        dlclose(handle1);
        dlclose(handle2);
        return 1;
    }

    // side_func2 returns x * 2
    res = side_func2(2);
    if (res != 4)
    {
        fprintf(stderr, "side_func returned unexpected value: %d\n", res);
        dlclose(handle1);
        dlclose(handle2);
        return 1;
    }

    dlclose(handle1);
    dlclose(handle2);

    return 0;
}