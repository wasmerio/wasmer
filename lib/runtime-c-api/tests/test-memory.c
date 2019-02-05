#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

int main()
{
    wasmer_memory_t *memory = NULL;
    wasmer_limits_t descriptor;
    descriptor.min = 10;
    descriptor.max = 10;
    wasmer_memory_result_t memory_result = wasmer_memory_new(&memory, descriptor);
    printf("Memory result:  %d\n", memory_result);
    assert(memory_result == WASMER_MEMORY_OK);

    uint32_t len = wasmer_memory_length(memory);
    printf("Memory pages length:  %d\n", len);
    assert(len == 10);

    printf("Destroy memory\n");
    wasmer_memory_destroy(memory);
    return 0;
}
