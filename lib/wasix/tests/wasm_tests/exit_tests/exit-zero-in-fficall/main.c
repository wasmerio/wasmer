#include <stdio.h>
#include <ffi.h>
#include <assert.h>
#include <stdlib.h>

void exit_with_code(void){
    printf("FFI call\n");
    exit(0); 
}

int main(){
    ffi_cif cif;

    ffi_type *arg_types[0];
    ffi_type *ret_type;
    ret_type = &ffi_type_void;
    
    ffi_status cif_result = ffi_prep_cif(&cif, FFI_DEFAULT_ABI, 0, ret_type, arg_types);
    if (cif_result != FFI_OK) {
        fprintf(stderr, "ffi_prep_cif failed with status %d\n", cif_result);
        return 1;
    }
    
    ffi_call(&cif, (void (*)(void))&exit_with_code, NULL, NULL);

    return 1;
}