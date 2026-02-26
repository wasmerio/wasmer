#include <stdio.h>

extern int get_value();

int main() {
    printf("The shared library returned: %i\n", get_value());
    return 0;
}