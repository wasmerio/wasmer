#include <stdio.h>
#include <time.h>

int main (int argc, char *argv[]) {
    printf("Hello wasmer!\n");
    time_t rawtime;
    struct tm *info;
    char buffer[80];

    time( &rawtime );

    info = localtime( &rawtime );

    printf("Almost!\n");
    printf("Current local time and date: %s\n", asctime(info));
    printf("Done!\n");

    return(0);
}

