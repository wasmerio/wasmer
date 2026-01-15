#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>
#include <stdint.h>
#include <wasi/api.h>

void test_multiple_calls() {
    printf("Test: Multiple successive calls (100 iterations)\n");
    // Based on Linux vDSO test (1000 iterations)

    const int iterations = 100;
    uint8_t prev_buf[32];
    memset(prev_buf, 0, sizeof(prev_buf));

    int ret_init = __wasi_random_get(prev_buf, sizeof(prev_buf));
    assert(ret_init == 0);

    for (int i = 1; i < iterations; i++) {
        uint8_t buf[32];
        int ret = __wasi_random_get(buf, sizeof(buf));
        assert(ret == 0);

        // Assert that each call produces different data
        assert(memcmp(buf, prev_buf, sizeof(buf)) != 0 &&
               "WASIX BUG: Successive calls produced IDENTICAL data");

        memcpy(prev_buf, buf, sizeof(buf));
    }

    printf("  ✓ All %d calls produced unique data\n", iterations);
}

void test_consistency_within_buffer() {
    printf("\nTest: Buffer is fully filled (not partial)\n");
    // Tests that random_get writes entire buffer, not just partial fill

    const size_t buf_size = 128;
    uint8_t buf[128];

    // Fill with known pattern
    memset(buf, 0xAA, sizeof(buf));

    int ret = __wasi_random_get(buf, buf_size);
    assert(ret == 0);

    // Check that entire buffer was written (no 0xAA bytes remaining)
    // Statistical note: probability of random 0xAA is 1/256 per byte
    // For 128 bytes, expected ~0.5 occurrences of 0xAA
    // So we allow up to 3 occurrences (well within statistical bounds)
    int pattern_count = 0;
    for (size_t i = 0; i < buf_size; i++) {
        if (buf[i] == 0xAA) {
            pattern_count++;
        }
    }

    // If more than 3 bytes are still 0xAA, likely a bug (probability < 0.1%)
    assert(pattern_count <= 3 && "WASIX BUG: Too many pattern bytes remain - buffer not fully filled");

    printf("  ✓ Buffer fully filled (found %d/128 random 0xAA bytes, within statistical bounds)\n", pattern_count);
}

void test_null_pointer() {
    printf("\nTest: NULL pointer handling\n");
    // Based on LTP getrandom01.c - should return EFAULT error

    int ret = __wasi_random_get(NULL, 100);

    // NULL pointer should return error (non-zero)
    assert(ret != 0 && "WASIX BUG: NULL pointer should return error, not success");

    printf("  ✓ NULL pointer correctly returns error (code: %d)\n", ret);
}

int main() {
    printf("=== random_get Integration Tests (C-only unique tests) ===\n\n");

    test_multiple_calls();
    test_consistency_within_buffer();
    test_null_pointer();

    printf("\n=== All random_get integration tests passed! ===\n");
    return 0;
}
