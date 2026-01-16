#include <stdio.h>
#include <stdlib.h>
#include <assert.h>
#include <stdint.h>

// WASIX closure syscalls - use wasi/api_wasix.h for declarations
#include <wasi/api_wasix.h>

// Test 1: closure_free without prior allocation (idempotent behavior)
// Based on: WASIX implementation - idempotent, always succeeds
void test_invalid_index() {
    printf("Test 1: closure_free with unallocated index (idempotent)\n");

    __wasi_function_pointer_t invalid_index = 999999;
    __wasi_errno_t ret = __wasi_closure_free(invalid_index);

    assert(ret == 0 && "closure_free should succeed (idempotent behavior)");
    printf("  ✓ Returned success (idempotent - safe to free unallocated)\n");
}

// Test 2: closure_allocate + closure_free (basic lifecycle)
// Based on: WASIX implementation - allocate then free should succeed
void test_allocate_and_free() {
    printf("\nTest 2: closure_allocate + closure_free (basic lifecycle)\n");

    __wasi_function_pointer_t closure_index = 0;

    // Allocate a closure slot
    __wasi_errno_t ret_alloc = __wasi_closure_allocate(&closure_index);
    printf("  closure_allocate returned: %d, index: %u\n", ret_alloc, closure_index);

    assert(ret_alloc == 0 && "closure_allocate should return 0 on success");
    assert(closure_index != 0 && "allocated index should be non-zero");

    // Free the allocated closure
    __wasi_errno_t ret_free = __wasi_closure_free(closure_index);
    printf("  closure_free(%u) returned: %d\n", closure_index, ret_free);

    assert(ret_free == 0 && "closure_free should return 0 for valid index");

    printf("  ✓ Allocate and free succeeded\n");
}

// Test 3: Double-free is safe (idempotent behavior)
// Based on: WASIX implementation - double-free succeeds (idempotent)
void test_double_free() {
    printf("\nTest 3: Double-free safety (idempotent)\n");

    __wasi_function_pointer_t closure_index = 0;
    __wasi_errno_t ret_alloc = __wasi_closure_allocate(&closure_index);

    assert(ret_alloc == 0 && "closure_allocate failed");

    // First free - should succeed
    __wasi_errno_t ret_free1 = __wasi_closure_free(closure_index);
    assert(ret_free1 == 0 && "first free should succeed");
    printf("  First free succeeded\n");

    // Second free - also succeeds (idempotent behavior)
    printf("  Attempting double-free (should also succeed)...\n");
    __wasi_errno_t ret_free2 = __wasi_closure_free(closure_index);

    printf("  Second free returned: %d\n", ret_free2);
    assert(ret_free2 == 0 && "double-free should succeed (idempotent)");
    printf("  ✓ Double-free succeeded (idempotent - safe behavior)\n");
}

// Test 4: Multiple allocate/free cycles
// Based on: stress-ng pattern - repeated operations should work
void test_multiple_cycles() {
    printf("\nTest 4: Multiple allocate/free cycles (10 iterations)\n");

    for (int i = 0; i < 10; i++) {
        uint32_t closure_index = 0;
        __wasi_errno_t ret_alloc = __wasi_closure_allocate(&closure_index);
        assert(ret_alloc == 0 && "closure_allocate failed in cycle");

        __wasi_errno_t ret_free = __wasi_closure_free(closure_index);
        assert(ret_free == 0 && "closure_free failed in cycle");
    }

    printf("  ✓ All 10 cycles succeeded\n");
}

// Test 5: closure_free with index 0 (idempotent)
// Based on: Edge case - index 0 is valid in WASM tables (0-indexed)
void test_index_zero() {
    printf("\nTest 5: closure_free with index 0 (idempotent)\n");

    // Index 0 is valid in WASM tables, probably not allocated
    __wasi_errno_t ret = __wasi_closure_free(0);
    printf("  closure_free(0) returned: %d\n", ret);

    assert(ret == 0 && "closure_free should succeed (idempotent)");
    printf("  ✓ Returned success (idempotent - safe even if not allocated)\n");
}

// Test 6: closure_free with maximum u32 value (idempotent)
// Based on: Boundary case - u32::MAX succeeds (idempotent)
void test_max_index() {
    printf("\nTest 6: closure_free with u32::MAX (idempotent)\n");

    uint32_t max_index = 0xFFFFFFFF;
    __wasi_errno_t ret = __wasi_closure_free(max_index);
    printf("  closure_free(0xFFFFFFFF) returned: %d\n", ret);

    assert(ret == 0 && "closure_free should succeed (idempotent)");
    printf("  ✓ Returned success (idempotent - safe for any index)\n");
}

// Test 7: Allocate multiple closures, free in different order
// Based on: Index management - verify proper bookkeeping
void test_multiple_allocations() {
    printf("\nTest 7: Multiple allocations, free in reverse order\n");

    const int count = 5;
    __wasi_function_pointer_t indices[count];

    // Allocate multiple closures
    for (int i = 0; i < count; i++) {
        __wasi_errno_t ret = __wasi_closure_allocate(&indices[i]);
        assert(ret == 0 && "allocation failed");
        printf("  Allocated closure %d: index %u\n", i, indices[i]);
    }

    // Verify all indices are unique
    for (int i = 0; i < count; i++) {
        for (int j = i + 1; j < count; j++) {
            assert(indices[i] != indices[j] &&
                   "allocated indices are not unique");
        }
    }
    printf("  ✓ All %d indices are unique\n", count);

    // Free in reverse order
    for (int i = count - 1; i >= 0; i--) {
        __wasi_errno_t ret = __wasi_closure_free(indices[i]);
        assert(ret == 0 && "free failed");
        printf("  Freed closure %d: index %u\n", i, indices[i]);
    }

    printf("  ✓ All closures freed successfully\n");
}

int main() {
    printf("WASIX closure_free Integration Tests\n");
    printf("=====================================\n\n");

    test_invalid_index();
    test_allocate_and_free();
    test_double_free();
    test_multiple_cycles();
    test_index_zero();
    test_max_index();
    test_multiple_allocations();

    printf("\n=====================================\n");
    printf("✓ All closure_free tests completed!\n");

    return 0;
}
