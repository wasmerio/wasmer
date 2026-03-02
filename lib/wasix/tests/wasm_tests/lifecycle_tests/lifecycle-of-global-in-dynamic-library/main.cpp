#include <dlfcn.h>
#include <thread>
#include <iostream>

int main() {
    void* handle = dlopen("liblibrary.so", RTLD_NOW);
    if (!handle) {
        std::cerr << "dlopen failed: " << dlerror() << std::endl;
        return 1;
    }

    dlclose(handle);
    return 0;
}
