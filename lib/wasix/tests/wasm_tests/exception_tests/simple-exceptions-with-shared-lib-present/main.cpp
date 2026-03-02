#include <stdio.h>
#include <assert.h>
#include "library.hpp"

int main() {
    try {
        // See if the presence of a shared library changes anything.
        int number = get_number_from_library();
        assert(number == 42);
        
        throw "An exception occurred!";
    } catch (const char* msg) {
        printf("Caught exception: %s\n", msg);
    }
    return 0;
}