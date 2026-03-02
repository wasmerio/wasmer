#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>

typedef char (*module_name_t)();

int main() {
    void *handle_a = dlopen("a/libside.so", RTLD_NOW | RTLD_LOCAL);
    assert(handle_a);

    module_name_t module_name_a = dlsym(handle_a, "module_name");
    assert(module_name_a);

    char name_a = module_name_a();
    printf("Module A returned: %c\n", name_a);
    assert(name_a == 'A');

    void *handle_b = dlopen("b/libside.so", RTLD_NOW | RTLD_LOCAL);
    assert(handle_b);

    module_name_t module_name_b = dlsym(handle_b, "module_name");
    assert(module_name_b);

    char name_b = module_name_b();
    printf("Module B returned: %c\n", name_b);
    assert(name_b == 'B');

    int result_a = dlclose(handle_a);
    assert(result_a == 0);

    int result_b = dlclose(handle_b);
    assert(result_b == 0);

    return 0;
}
