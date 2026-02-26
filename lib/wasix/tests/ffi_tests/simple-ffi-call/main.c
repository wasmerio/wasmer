#include <stdio.h>
#include <ffi.h>
#include <assert.h>

int fib(int n){
    if (n <= 1) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

// If you forgot the type
// Of your function pointer
// How you gonna call?
// (libffi!)
void *opaque_fib = &fib;

int main(){
    ffi_cif cif;

    ffi_type *arg_types[1];
    ffi_type *ret_type;
    arg_types[0] = &ffi_type_sint32;
    ret_type = &ffi_type_sint32;
    
    // To define a type
    // For your void*
    // Who you gonna call?
    // (ffi_prep_cif!)
    ffi_status cif_result = ffi_prep_cif(&cif, FFI_DEFAULT_ABI, 1, ret_type, arg_types);
    if (cif_result != FFI_OK) {
        fprintf(stderr, "ffi_prep_cif failed with status %d\n", cif_result);
        return 1;
    }
    
    int argument;
    int result;
    void *arg_values[1];
    void *ret_value;
    arg_values[0] = &argument;
    ret_value = &result;

    argument = 11;
    ffi_call(&cif, (void (*)(void))opaque_fib, ret_value, arg_values);

    printf("ffi_call returned %d\n", result);
    assert(result == 89);
    return 0;
}