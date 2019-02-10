#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

int main()
{
    wasmer_memory_t *memory = NULL;
    wasmer_limits_t descriptor;
    descriptor.min = 10;
    descriptor.max = 15;
    wasmer_memory_result_t memory_result = wasmer_memory_new(&memory, descriptor);
    printf("Memory result:  %d\n", memory_result);
    assert(memory_result == WASMER_MEMORY_OK);

    uint32_t len = wasmer_memory_length(memory);
    printf("Memory pages length:  %d\n", len);
    assert(len == 10);

    wasmer_memory_result_t grow_result = wasmer_memory_grow(memory, 2);
    assert(grow_result == WASMER_MEMORY_OK);

    uint32_t new_len =  wasmer_memory_length(memory);
    printf("Memory pages length:  %d\n", new_len);
    assert(new_len == 12);

    uint32_t bytes_len =  wasmer_memory_data_length(memory);
    printf("Memory bytes length:  %d\n", bytes_len);
    assert(bytes_len  == 12 * 65536);

    // Err, grow beyond max
    wasmer_memory_result_t grow_result2 = wasmer_memory_grow(memory, 10);
    assert(grow_result2 == WASMER_MEMORY_ERROR);

    printf("Destroy memory\n");
    wasmer_memory_destroy(memory);
    return 0;
}
