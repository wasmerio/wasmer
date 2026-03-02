#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>

typedef void (*cpp_function_t)();

int main() {
    void *handle = dlopen("liblibrary.so", RTLD_NOW | RTLD_LOCAL);
    assert(handle);

    cpp_function_t cpp_function = dlsym(handle, "cpp_function");
    assert(cpp_function);

    cpp_function();

    int result = dlclose(handle);
    assert(result == 0);

    return 0;
}
