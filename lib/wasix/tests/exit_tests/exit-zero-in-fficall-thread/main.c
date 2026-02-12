#include <stdio.h>
#include <ffi.h>
#include <assert.h>
#include <stdlib.h>
#include <pthread.h>
#include <unistd.h>

void exit_with_code(void){
    printf("FFI call in thread\n");
    exit(0);
}

void * thread_func(void * data){
    ffi_cif cif;

    ffi_type *arg_types[0];
    ffi_type *ret_type;
    ret_type = &ffi_type_void;
    
    ffi_status cif_result = ffi_prep_cif(&cif, FFI_DEFAULT_ABI, 0, ret_type, arg_types);
    if (cif_result != FFI_OK) {
        fprintf(stderr, "ffi_prep_cif failed with status %d\n", cif_result);
        exit(1);
    }
    
    ffi_call(&cif, (void (*)(void))&exit_with_code, NULL, NULL);

    return 0;
}

int main()
{
    pthread_attr_t attr = {0};
    if (pthread_attr_init(&attr) != 0) {
        perror("init attr");
        return -1;
    }

    pthread_t thread = {0};
    if (pthread_create(&thread, &attr, &thread_func, (void *)stdout) != 0) {
        perror("create thread");
        return -1;
    }

    void *thread_ret;
    if (pthread_join(thread, &thread_ret) != 0) {
        perror("join");
        return -1;
    }
    sleep(1);

    return 1;
}