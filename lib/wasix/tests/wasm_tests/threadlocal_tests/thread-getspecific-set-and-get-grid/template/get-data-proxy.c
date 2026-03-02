#if defined(GET_DATA_DIRECT) || defined(GET_DATA_SHARED)
#include "get-data.h"

void get_data_proxy() {
    get_data();
}
#elif defined(GET_DATA_DYNAMIC)
#include <dlfcn.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include "thread-keys.h"

typedef void (*get_data_func_t)();

void get_data_proxy() {
    void* handle = dlopen("./libget-data.so", RTLD_LAZY);
    if(handle == NULL) {
        fprintf(stderr, "dlopen failed: %s\n", dlerror());
        exit(1);
    }
    get_data_func_t func = (get_data_func_t)dlsym(handle, "get_data");
    if (func == NULL) {
        fprintf(stderr, "dlsym failed: %s\n", dlerror());
        exit(1);
    }
    func();
    dlclose(handle);
}
#else 
#error "You need to define one of GET_DATA_DIRECT, GET_DATA_SHARED, or GET_DATA_DYNAMIC"
#endif