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
    wasmer_result_t memory_result = wasmer_memory_new(&memory, descriptor);
    printf("Memory result:  %d\n", memory_result);
    assert(memory_result == WASMER_OK);

    uint32_t len = wasmer_memory_length(memory);
    printf("Memory pages length:  %d\n", len);
    assert(len == 10);

    wasmer_result_t grow_result = wasmer_memory_grow(memory, 2);
    assert(grow_result == WASMER_OK);

    uint32_t new_len =  wasmer_memory_length(memory);
    printf("Memory pages length:  %d\n", new_len);
    assert(new_len == 12);

    uint32_t bytes_len =  wasmer_memory_data_length(memory);
    printf("Memory bytes length:  %d\n", bytes_len);
    assert(bytes_len  == 12 * 65536);

    // Err, grow beyond max
    wasmer_result_t grow_result2 = wasmer_memory_grow(memory, 10);
    assert(grow_result2 == WASMER_ERROR);
//    int error_len = wasmer_last_error_length();
//    char *error_str = malloc(error_len);
//    wasmer_last_error_message(error_str, error_len);
//    assert(0 == strcmp(error_str, "Creation error"));
//    free(error_str);


//    wasmer_memory_t *bad_memory = NULL;
//    wasmer_limits_t bad_descriptor;
//    bad_descriptor.min = 15;
//    bad_descriptor.max = 10;
//    wasmer_result_t bad_memory_result = wasmer_memory_new(&bad_memory, bad_descriptor);
//    printf("Bad memory result:  %d\n", bad_memory_result);
//    assert(memory_result == WASMER_MEMORY_ERROR);
//
//    int error_len = wasmer_last_error_length();
//    char *error_str = malloc(error_len);
//    wasmer_last_error_message(error_str, error_len);
//    assert(0 == strcmp(error_str, "Creation error"));
//    free(error_str);

    printf("Destroy memory\n");
    wasmer_memory_destroy(memory);
    return 0;
}
