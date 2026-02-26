#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>

int main(void) {
    size_t size = 4096;
    char *p = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_ANON | MAP_PRIVATE, -1, 0);
    if (p == MAP_FAILED) {
        perror("mmap");
        return 1;
    }
    const char *msg = "mmap anon memory works";
    strcpy(p, msg);
    if (strcmp(p, msg) != 0) {
        fprintf(stderr, "readback mismatch\n");
        munmap(p, size);
        return 1;
    }
    printf("%s\n", p);
    if (munmap(p, size) != 0) {
        perror("munmap");
        return 1;
    }
    return 0;
}
