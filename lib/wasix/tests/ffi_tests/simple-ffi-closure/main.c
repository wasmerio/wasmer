#include <stdio.h>
#include <ffi.h>
#include <assert.h>

// Callback function for closure
static void closure_callback(ffi_cif* cif, void* ret, void* args[], void* user_data) {
    // Get the call counter from user data
    int* count = (int*)user_data;
    
    // Get the function arguments
    int a = *(int*)args[0];
    int b = *(int*)args[1];
    
    // Print debug information
    printf("Inside closure callback: %d + %d (called %d times)\n", a, b, *count);
    
    // Calculate result: a + b + call_count
    *(int*)ret = a + b + (*count)++;
}

int main() {
    ffi_status status;
    
    printf("=== Testing libffi closures ===\n");
    
    // Initialize the call interface
    ffi_cif cif;
    ffi_type *arg_types[2];
    ffi_closure *closure;
    
    // This will hold our callable function pointer
    int (*closure_func)(int, int);
    
    // This will count how many times the closure is called
    int call_count = 0;
    
    // Allocate a closure
    closure = ffi_closure_alloc(sizeof(ffi_closure), (void**)&closure_func);
    if (!closure) {
        fprintf(stderr, "Failed to allocate closure\n");
        return 1;
    }
    
    // Set up argument and return types
    ffi_type *ret_type = &ffi_type_sint32;
    arg_types[0] = &ffi_type_sint32;  // First argument (int)
    arg_types[1] = &ffi_type_sint32;  // Second argument (int)
    
    // Prepare the call interface
    status = ffi_prep_cif(&cif, FFI_DEFAULT_ABI, 2, ret_type, arg_types);
    if (status != FFI_OK) {
        fprintf(stderr, "Failed to prepare CIF\n");
        return 1;
    }
    
    // Initialize the closure
    status = ffi_prep_closure_loc(
        closure,           // The closure
        &cif,              // The call interface
        closure_callback,  // The callback function
        &call_count,       // User data (passed to callback)
        closure_func       // The function pointer to call
    );
    
    if (status != FFI_OK) {
        fprintf(stderr, "Failed to prepare closure\n");
        return 1;
    }
    
    printf("Testing closure...\n");
    
    // First call
    int result1 = closure_func(10, 20);
    printf("Closure result 1: %d\n", result1);
    assert(result1 == 30);  // 10 + 20 + 0
    
    // Second call (should increment the counter)
    int result2 = closure_func(5, 7);
    printf("Closure result 2: %d\n", result2);
    assert(result2 == 13);  // 5 + 7 + 1
    
    // Third call (should increment the counter again)
    int result3 = closure_func(100, 200);
    printf("Closure result 3: %d\n", result3);
    assert(result3 == 302); // 100 + 200 + 2
    
    // Clean up
    ffi_closure_free(closure);
    
    printf("Closure test completed\n");
    return 0;
}
