#include <stdio.h>
#include <stdlib.h>

typedef void (* cool_fn_type)();
void cool_fn_impl() {
    printf("called\n");
}

void side() {
	cool_fn_type cool_fn =  cool_fn_impl;
	cool_fn();
}
