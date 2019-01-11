#include <stdio.h>
#include <time.h>

// a simple program that simply calls clock_gettime
// not easy to unit test because of non-determinism and so compilation is enough for now
int main () {
    struct timespec tp;
    clock_gettime(CLOCK_REALTIME, &tp);
    printf("clock_gettime\n");
}
