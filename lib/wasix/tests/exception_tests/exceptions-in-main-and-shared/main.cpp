#include <stdio.h>
#include <assert.h>
#include "library.hpp"

int try_catch_in_main() {
    try {
        throw "An exception occurred!";
    } catch (...) {
        printf("Caught some exception\n");
    }
    return 42;
}

int main() {
    try_catch_in_main();
    try_catch_in_lib();
    return 0;
}