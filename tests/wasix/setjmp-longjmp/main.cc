#include <stdio.h>
#include <stdlib.h>
#include <setjmp.h>
#include <sys/types.h>

int main()
{
    // Put variables on both ends to make sure the setjmp
    // code doesn't overwrite anything by mistake
    unsigned long long int before = 10;
    jmp_buf jmp;
    unsigned long long int after = 20;

    if (setjmp(jmp) == 0)
    {
        if (before != 10 || after != 20)
        {
            printf("oops 1\n");
            exit(1);
        }
        longjmp(jmp, 1);
        printf("oops 2\n");
        exit(2);
    }
    else
    {
        if (before != 10 || after != 20)
        {
            printf("oops 3\n");
            exit(3);
        }
        before = 50;
        after = 60;
    }

    if (before != 50 || after != 60)
    {
        printf("oops 4\n");
        exit(4);
    }

    return 0;
}