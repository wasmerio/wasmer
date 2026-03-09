#include <stdio.h>

__attribute__((constructor))
static void init() {
    printf("b");
}

__attribute__((destructor))
static void fini() {
    printf("d");
}
