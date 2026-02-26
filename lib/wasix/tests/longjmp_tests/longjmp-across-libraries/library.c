#include <stdio.h>
#include <setjmp.h>

extern jmp_buf buffer;

void other() {
    printf("b");
    longjmp(buffer, 1);
    printf("This line will never be executed\n");
}
