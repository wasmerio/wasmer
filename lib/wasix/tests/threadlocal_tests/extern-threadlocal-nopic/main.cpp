#include <stdio.h>

extern thread_local int erryes;

int main() {
    printf("error number: %i\n", erryes);
    return 0;
}