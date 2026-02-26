#include <cstdio>
#include "thrower.hpp"

void catch_exception() {
    try {
        throw_exception();
    } catch (const char* msg) {
        printf("Caught exception: %s\n", msg);
    }
}