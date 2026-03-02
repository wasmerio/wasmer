#include <stdio.h>

_Thread_local int my_tls_int = 42;
extern int get_value();

int main() {
    printf("The shared library returned: %i\n", get_value());
    return 0;
}