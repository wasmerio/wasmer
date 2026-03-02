#include <assert.h>
#include <stdlib.h>
#include <dlfcn.h>
#include <stdio.h>

__attribute__((__weak__)) extern int other_func();

int main() {
	if (!other_func) {
		printf("other_func is not defined, but the program still compiled\n");
		return 0;
	}
	int result = other_func();
	printf("other_func returned %i\n",result);
	return 0;
}
