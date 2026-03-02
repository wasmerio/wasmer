#include <stdio.h>
#include <assert.h>
#include "library.hpp"

void throw_exception() {
    throw "An exception occurred!";
}

int main() {
    try {
        // This function calls throw_exception() from the shared library.
        int number = get_number_from_library();
        assert(number == 42);
    } catch (const char* msg) {
        printf("Caught exception: %s\n", msg);
    }
    return 0;
}