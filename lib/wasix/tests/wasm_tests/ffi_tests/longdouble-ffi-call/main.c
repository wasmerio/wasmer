#include <stdio.h>
#include <ffi.h>
#include <assert.h>
#include <string.h>

long double multiply_ldoubles(long double a, long double b) {
    return a * b;
}

void *opaque_multiply_ldoubles = &multiply_ldoubles;

int main() {
    ffi_status status;
    void *arg_values[10];
    
    // Test 3: Long double operations
    {
        ffi_cif ld_cif;
        ffi_type *ld_arg_types[2] = { &ffi_type_longdouble, &ffi_type_longdouble };
        
        status = ffi_prep_cif(&ld_cif, FFI_DEFAULT_ABI, 2, &ffi_type_longdouble, ld_arg_types);
        if (status != FFI_OK) {
            fprintf(stderr, "Failed to prepare long double CIF\n");
            return 1;
        }
        
        long double a = 123456789.123456789L;
        long double b = 987654321.987654321L;
        long double result;
        
        void *ld_args[2] = { &a, &b };
        ffi_call(&ld_cif, (void (*)(void))opaque_multiply_ldoubles, &result, ld_args);
        
        // Calculate expected result with high precision
        long double expected = a * b;
        printf("Long double result: %Lf\n", result);
        printf("Expected: %Lf\n", expected);
        
        // Compare with a small epsilon due to floating-point imprecision
        long double epsilon = 0.000001L;
        long double diff = result - expected;
        if (diff < 0) diff = -diff;
        assert(diff < epsilon);
    }
    
    printf("\nAll tests passed!\n");
    return 0;
}
