#include <stdio.h>
#include <time.h>

int main (int argc, char *argv[]) {
    time_t rawtime;
    struct tm *info;
    time( &rawtime );
    info = localtime( &rawtime );
    struct tm info2;
    time (&rawtime );
    struct tm *p = localtime_r(&rawtime, &info2);
    printf("localtime\n");
    return(0);
}

