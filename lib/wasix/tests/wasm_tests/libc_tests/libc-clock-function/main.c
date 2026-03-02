#include <stdio.h>
#include <time.h>

// Busy-wait loop for demonstration
void busy_work() {
    volatile double x = 0.0;
    for (long i = 0; i < 10000000; ++i) {
        x += i * 0.000001;
    }
}

int main() {
    clock_t start = clock();

    busy_work();

    clock_t end = clock();

    double cpu_time_used = ((double)(end - start)) / CLOCKS_PER_SEC;

    if (cpu_time_used > 0.0) {
        printf("Clock works.\n");
        return 0;
    } else {
        printf("Test failed: No CPU time recorded.\n");
        return 1;
    }
}
