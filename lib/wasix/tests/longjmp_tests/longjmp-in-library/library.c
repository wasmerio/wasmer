#include <stdio.h>
#include <setjmp.h>

jmp_buf buffer;

void other() {
    printf("b");
    longjmp(buffer, 1);
    printf("This line will never be executed\n");
}

void lib_main() {
    if (setjmp(buffer) == 0) {
        // Initial call to setjmp returns 0
        printf("a");
        other();
    } else {
        printf("c\n");
    }
}
