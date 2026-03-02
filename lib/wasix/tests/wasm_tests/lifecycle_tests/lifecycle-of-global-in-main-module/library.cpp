#include <iostream>

struct Item {
    Item() { std::cout << "Item constructed"; }
    ~Item() { std::cout << "Item destructed"; }
};
struct TlsItem {
    TlsItem() { std::cout << "TlsItem constructed"; }
    ~TlsItem() { std::cout << "TlsItem destructed"; }
};

thread_local TlsItem tls_item;
Item item;

extern "C" void* use_tls_item() {
    return &tls_item;
}