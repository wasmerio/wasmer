#include <stdio.h>
#include <setjmp.h>
#include "library.h"

extern jmp_buf buffer;

int main() {
    if (setjmp(buffer) == 0) {
        // Initial call to setjmp returns 0
        printf("a");
        other();
    } else {
        printf("c\n");
    }
    return 0;
}
