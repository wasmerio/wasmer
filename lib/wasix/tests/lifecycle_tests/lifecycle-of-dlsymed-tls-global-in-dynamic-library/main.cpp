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

    int * tls_item_loaded = (int *)dlsym(handle, "tls_item");
    if (!tls_item_loaded) {
        printf("dlsym failed: %s\n", dlerror());
        return 1;
    }
    // In the default TLS model this should neither construct nor destruct the tls item, but just call test on an uninitialized item
    int _ = *tls_item_loaded;

    dlclose(handle);
    return 0;
}
