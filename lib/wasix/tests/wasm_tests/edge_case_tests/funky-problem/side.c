#include <stdio.h>
#include <stdlib.h>

typedef void (* cool_fn_type)();

void cool_fn_impl() {
    printf("called\n");
}

volatile cool_fn_type* keep;

void repro() {
	cool_fn_type cool_fn =  cool_fn_impl;
	
	int before = !!cool_fn;
	// Take the address and assign it to a global variable to prevent
    // the compiler from optimizing it away.
	keep = &cool_fn;
    // Printing afterwards is neccessary. Maybe any call to the main module works?
	printf(".");
	int after = !!cool_fn;
	if (before == after) {
		printf("Nothing weird happened\n");
	} else {
		printf("Something weird happened\n");
	}
}

int side() {
    repro();
    return 0;
}
