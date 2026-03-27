#include <stdio.h>
#include <ffi.h>
#include <assert.h>
#include <string.h>

// Test 1: Basic struct passing and returning
typedef struct {
    int x;
    double y;
    char z[32];
} TestStruct;

TestStruct update_struct(TestStruct s) {
    s.x += 1;
    s.y *= 2.0;
    strcat(s.z, "_updated");
    return s;
}

// Test 2: Function with 10 arguments
int sum_ten(int a, int b, int c, int d, int e, 
            int f, int g, int h, int i, int j) {
    return a + b + c + d + e + f + g + h + i + j;
}

// Test 3: Long double operations
long double multiply_ldoubles(long double a, long double b) {
    return a * b;
}

// Function that creates and returns a new struct
TestStruct create_struct(int x, double y, const char* z) {
    TestStruct result;
    result.x = x;
    result.y = y;
    strncpy(result.z, z, sizeof(result.z) - 1);
    result.z[sizeof(result.z) - 1] = '\0'; // Ensure null termination
    printf("Inside create_struct: {%d, %f, %s}\n", result.x, result.y, result.z);
    return result;
}

// Opaque function pointers
void *opaque_update_struct = &update_struct;
void *opaque_sum_ten = &sum_ten;
void *opaque_multiply_ldoubles = &multiply_ldoubles;
void *opaque_create_struct = &create_struct;

int main() {
    ffi_status status;
    void *arg_values[10];
    
    // Test 1: Struct passing and returning
    {
        printf("=== Testing struct passing/returning ===\n");
        ffi_cif struct_cif;
        ffi_type *struct_arg_types[1];
        
        // Define struct type
        ffi_type struct_type;
        ffi_type *struct_elements[7];
        
        // Initialize the struct type
        memset(&struct_type, 0, sizeof(ffi_type));
        struct_type.type = FFI_TYPE_STRUCT;
        
        // Set up elements
        struct_elements[0] = &ffi_type_sint32;  // x
        struct_elements[1] = &ffi_type_double;  // y
        struct_elements[2] = &ffi_type_uint64;   // z[0] - treat char array as bytes
        struct_elements[3] = &ffi_type_uint64;   // z[0] - treat char array as bytes
        struct_elements[4] = &ffi_type_uint64;   // z[0] - treat char array as bytes
        struct_elements[5] = &ffi_type_uint64;   // z[0] - treat char array as bytes
        struct_elements[6] = NULL;              // Terminator
        
        struct_type.elements = struct_elements;
        
        struct_arg_types[0] = &struct_type;
        
        status = ffi_prep_cif(&struct_cif, FFI_DEFAULT_ABI, 1, &struct_type, struct_arg_types);
        if (status != FFI_OK) {
            fprintf(stderr, "Failed to prepare struct CIF\n");
            return 1;
        }
        
        // Prepare test data
        TestStruct test_in;
        TestStruct test_out;
        
        // Initialize test data
        memset(&test_in, 0, sizeof(TestStruct));
        test_in.x = 42;
        test_in.y = 3.14;
        strncpy(test_in.z, "test_string", sizeof(test_in.z) - 1);
        
        // Print struct size and alignment for debugging
        printf("TestStruct size: %zu, alignment: %zu\n", 
               sizeof(TestStruct), __alignof__(TestStruct));
        printf("TestStruct field offsets: x=%zu, y=%zu, z=%zu\n",
               (size_t)&((TestStruct *)0)->x,
               (size_t)&((TestStruct *)0)->y,
               (size_t)&((TestStruct *)0)->z);
               
        // Initialize output
        memset(&test_out, 0, sizeof(TestStruct));
        
        // For struct passing, we need to pass pointers
        void *args[1] = { &test_in };
        
        ffi_call(&struct_cif, (void (*)(void))opaque_update_struct, &test_out, args);
        
        printf("Struct test: {%d, %f, %s}\n", test_out.x, test_out.y, test_out.z);
        assert(test_out.x == 43);
        assert(test_out.y > 6.2799 && test_out.y < 6.2801);
        assert(strcmp(test_out.z, "test_string_updated") == 0);
    }
    
    // Test 2: 10 arguments
    {
        printf("\n=== Testing 10 arguments ===\n");
        ffi_cif ten_args_cif;
        ffi_type *ten_args_types[10];
        
        for (int i = 0; i < 10; i++) {
            ten_args_types[i] = &ffi_type_sint32;
        }
        
        status = ffi_prep_cif(&ten_args_cif, FFI_DEFAULT_ABI, 10, &ffi_type_sint32, ten_args_types);
        if (status != FFI_OK) {
            fprintf(stderr, "Failed to prepare 10-args CIF\n");
            return 1;
        }
        
        int args[10] = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10};
        for (int i = 0; i < 10; i++) {
            arg_values[i] = &args[i];
        }
        
        int result;
        ffi_call(&ten_args_cif, (void (*)(void))opaque_sum_ten, &result, arg_values);
        
        printf("Sum of 1..10: %d\n", result);
        assert(result == 55);
    }
    
    // Test 3: Long double operations
    {
        printf("\n=== Testing long double operations ===\n");
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
    
    // Test 4: Return a struct from a function
    {
        printf("\n=== Testing returning a struct ===\n");
        ffi_cif create_cif;
        ffi_type *create_arg_types[3];
        
        // Set up argument types
        create_arg_types[0] = &ffi_type_sint32;  // x
        create_arg_types[1] = &ffi_type_double;  // y
        create_arg_types[2] = &ffi_type_pointer; // z
        
        // Define the return type (same as before)
        ffi_type return_struct_type;
        ffi_type *return_struct_elements[7];
        
        memset(&return_struct_type, 0, sizeof(ffi_type));
        return_struct_type.type = FFI_TYPE_STRUCT;
        return_struct_elements[0] = &ffi_type_sint32;  // x
        return_struct_elements[1] = &ffi_type_double;  // y
        return_struct_elements[2] = &ffi_type_uint64;  // z[0-7]
        return_struct_elements[3] = &ffi_type_uint64;  // z[8-15]
        return_struct_elements[4] = &ffi_type_uint64;  // z[16-23]
        return_struct_elements[5] = &ffi_type_uint64;  // z[24-31]
        return_struct_elements[6] = NULL;             // Terminator
        return_struct_type.elements = return_struct_elements;
        
        // Prepare the call interface
        status = ffi_prep_cif(&create_cif, FFI_DEFAULT_ABI, 3, &return_struct_type, create_arg_types);
        if (status != FFI_OK) {
            fprintf(stderr, "Failed to prepare create_struct CIF\n");
            return 1;
        }
        
        // Prepare arguments
        int x = 100;
        double y = 3.14159;
        const char *z = "created_by_ffi";
        void *create_args[3] = { &x, &y, &z };
        
        // Call the function through FFI
        TestStruct result;
        ffi_call(&create_cif, (void (*)(void))opaque_create_struct, &result, create_args);
        
        // Verify the result
        printf("Returned struct: {%d, %f, %s}\n", result.x, result.y, result.z);
        assert(result.x == 100);
        assert(result.y > 3.14158 && result.y < 3.14160);
        assert(strcmp(result.z, "created_by_ffi") == 0);
    }
    
    printf("\nAll tests passed!\n");
    return 0;
}
