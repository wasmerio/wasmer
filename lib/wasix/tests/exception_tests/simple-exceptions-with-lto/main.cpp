#include <stdio.h>

int main() {
    try {
        throw "An exception occurred!";
    } catch (const char* msg) {
        printf("Caught exception: %s\n", msg);
    }
    return 0;
}