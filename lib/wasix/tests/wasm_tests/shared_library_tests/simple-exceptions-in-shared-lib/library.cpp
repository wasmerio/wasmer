#include <stdio.h>

int try_catch_in_lib() {
    try {
        throw "An exception occurred!";
    } catch (const char* msg) {
        printf("Caught exception: %s\n", msg);
    }
    return 42;
}