#include <pthread.h>
#include "thread-keys.h"
#include <assert.h>
#include <stdio.h>
#if defined(SET_DATA_PROXY_DIRECT) || defined(SET_DATA_PROXY_SHARED)
#include "set-data-proxy.h"
#endif
#if defined(GET_DATA_PROXY_DIRECT) || defined(GET_DATA_PROXY_SHARED)
#include "get-data-proxy.h"
#endif
#if defined(SET_DATA_PROXY_DYNAMIC) || defined(GET_DATA_PROXY_DYNAMIC)
#include "dlfcn.h"
#include <stdlib.h>
#endif

pthread_key_t key_a;
pthread_key_t key_b;

void* run_test(void *data) {
    // Set thread specific data
#if defined(SET_DATA_PROXY_DYNAMIC)
    void* handle_set_data_proxy = dlopen("./libset-data-proxy.so", RTLD_LAZY);
    if(handle_set_data_proxy == NULL) {
        fprintf(stderr, "dlopen failed: %s\n", dlerror());
        exit(1);
    }
    typedef void (*set_data_proxy_func_t)();
    set_data_proxy_func_t set_data_proxy = (set_data_proxy_func_t)dlsym(handle_set_data_proxy, "set_data_proxy");
    if (set_data_proxy == NULL) {
        fprintf(stderr, "dlsym failed: %s\n", dlerror());
        exit(1);
    }
#endif
    set_data_proxy();

    // Get thread specific data
#if defined(GET_DATA_PROXY_DYNAMIC)
    void* handle_get_data_proxy = dlopen("./libget-data-proxy.so", RTLD_LAZY);
    if(handle_get_data_proxy == NULL) {
        fprintf(stderr, "dlopen failed: %s\n", dlerror());
        exit(1);
    }
    typedef void (*get_data_proxy_func_t)();
    get_data_proxy_func_t get_data_proxy = (get_data_proxy_func_t)dlsym(handle_get_data_proxy, "get_data_proxy");
    if (get_data_proxy == NULL) {
        fprintf(stderr, "dlsym failed: %s\n", dlerror());
        exit(1);
    }
#endif
    get_data_proxy();

    // Terminate the output with a newline
    printf("\n");
}

int main() {
    int res = pthread_key_create(&key_a, NULL);
    assert(res == 0);
    assert(pthread_getspecific(key_a) == NULL);
    res = pthread_key_create(&key_b, NULL);
    assert(res == 0);
    assert(pthread_getspecific(key_b) == NULL);

#if defined(THREAD_WORKER)
    pthread_attr_t attr = {0};
    if (pthread_attr_init(&attr) != 0) {
        perror("init attr");
        return -1;
    }

    pthread_t thread = {0};
    if (pthread_create(&thread, &attr, &run_test, (void *)stdout) != 0) {
        perror("create thread");
        return -1;
    }

    void *thread_ret;
    if (pthread_join(thread, &thread_ret) != 0) {
        perror("join");
        return -1;
    }
#elif defined(THREAD_MAIN)
    run_test(NULL);
#else
#error "You need to define one of THREAD_MAIN or THREAD_WORKER"
#endif

    pthread_key_delete(key_a);
    pthread_key_delete(key_b);
    return 0;
}
