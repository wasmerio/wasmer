#include <stdio.h>

int try_catch_in_lib() {
    try {
        throw "An exception occurred!";
    } catch (...) {
        printf("Caught some exception\n");
    }
    return 42;
}