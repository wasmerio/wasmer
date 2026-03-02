#include <assert.h>
#include <stdlib.h>
#include <dlfcn.h>
#include <stdio.h>

__attribute__((__weak__)) extern int other_func();

int main() {
	if (!other_func) {
		printf("other_func is not defined\n");
		exit(1);
	}
	int result = other_func();
	printf("other_func returned %i\n",result);
	exit(0);
}
