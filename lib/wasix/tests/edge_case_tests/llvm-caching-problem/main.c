#include <assert.h>
#include <stdlib.h>
#include <dlfcn.h>
#include <stdio.h>

int main()
{
    void *handle = dlopen("libside.so", RTLD_NOW);
    assert(handle != NULL);
    int (*get_value)() = (int (*)())dlsym(handle, "get_value");
    assert(get_value != NULL);
    int value = get_value();
	printf("The dynamic library returned: %d\n", value);
    assert(value == 42);
}
