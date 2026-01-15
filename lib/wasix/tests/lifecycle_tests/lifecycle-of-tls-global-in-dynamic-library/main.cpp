#include <dlfcn.h>
#include <thread>
#include <iostream>

typedef void * (* side_type)();
int main() {
    void* handle = dlopen("liblibrary.so", RTLD_NOW);
    if (!handle) {
        std::cerr << "dlopen failed: " << dlerror() << std::endl;
        return 1;
    }

    side_type side = (side_type)dlsym(handle, "use_tls_item");
    void * (*use_tls_item)() = (void * (*)())dlsym(handle, "use_tls_item");
    if (!side) {
        printf("dlsym failed: %s\n", dlerror());
        return 1;
    }
    use_tls_item();

    dlclose(handle);
    return 0;
}
