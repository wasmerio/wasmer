#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>

__attribute__((constructor))
static void init() {
    printf("a");
}

__attribute__((destructor))
static void fini() {
    printf("f");
}

int main() {
    printf("c");

    void *handle = dlopen("libside.so", RTLD_NOW | RTLD_LOCAL);
    assert(handle);

    int result = dlclose(handle);
    assert(result == 0);

    printf("e");

    return 0;
}
