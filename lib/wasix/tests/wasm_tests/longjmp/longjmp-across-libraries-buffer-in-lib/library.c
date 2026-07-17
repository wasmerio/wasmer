#include <setjmp.h>
#include <stdio.h>

jmp_buf buffer;

void other() {
  printf("b");
  longjmp(buffer, 1);
  printf("This line will never be executed\n");
}
