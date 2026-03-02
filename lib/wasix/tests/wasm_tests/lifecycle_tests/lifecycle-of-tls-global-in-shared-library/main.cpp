#include <dlfcn.h>
#include <thread>
#include <iostream>

extern "C" void* use_tls_item();

int main() {
    // For the same test, but without the TLS item beeing used, see lifecycle-of-global-in-shared-library
    use_tls_item();
    return 0;
}
