#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>

typedef int (*get_value_t)();

int main() {
    void *handle = dlopen("libside.so", RTLD_NOW | RTLD_LOCAL);
    assert(handle);

    get_value_t get_value = dlsym(handle, "get_value");
    assert(get_value);

    int side_value = get_value();
    printf("The shared library returned: %i\n", side_value);
    assert(side_value == 42);

    int result = dlclose(handle);
    assert(result == 0);

    return 0;
}
