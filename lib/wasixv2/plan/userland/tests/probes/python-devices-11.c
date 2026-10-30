#include <stdio.h>
#include <unistd.h>
int main(void) {
    printf("ac_cv_file__dev_ptmx=%s\n", access("/dev/ptmx", F_OK) ? "no" : "yes");
    printf("ac_cv_file__dev_ptc=%s\n", access("/dev/ptc", F_OK) ? "no" : "yes");
    return 0;
}
