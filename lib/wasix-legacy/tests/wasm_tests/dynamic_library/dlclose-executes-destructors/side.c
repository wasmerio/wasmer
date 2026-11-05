#include <stdio.h>

// The loader runs this during `dlopen`, before control returns to `main`.
__attribute__((constructor)) static void init() { printf("b"); }

// The loader runs this during `dlclose`, before `dlclose` returns to `main`.
__attribute__((destructor)) static void fini() { printf("d"); }
