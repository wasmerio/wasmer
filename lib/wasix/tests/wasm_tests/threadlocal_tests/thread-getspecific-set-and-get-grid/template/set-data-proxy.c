#if defined(SET_DATA_DIRECT) || defined(SET_DATA_SHARED)
#include "set-data.h"

void set_data_proxy() {
    set_data();
}
#elif defined(SET_DATA_DYNAMIC)
#include <dlfcn.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include "thread-keys.h"

typedef void (*set_data_func_t)();

void set_data_proxy() {
    void* handle = dlopen("./libset-data.so", RTLD_LAZY);
    if(handle == NULL) {
        fprintf(stderr, "dlopen failed: %s\n", dlerror());
        exit(1);
    }
    set_data_func_t func = (set_data_func_t)dlsym(handle, "set_data");
    if (func == NULL) {
        fprintf(stderr, "dlsym failed: %s\n", dlerror());
        exit(1);
    }
    func();
    dlclose(handle);
}
#else 
#error "You need to define one of SET_DATA_DIRECT, SET_DATA_SHARED, or SET_DATA_DYNAMIC"
#endif