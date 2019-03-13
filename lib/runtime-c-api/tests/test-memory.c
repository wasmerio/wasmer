#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>

int main()
{
    wasmer_memory_t *memory = NULL;
    wasmer_limits_t descriptor;
    descriptor.min = 10;
    wasmer_limit_option_t max;
    max.has_some = true;
    max.some = 15;
    descriptor.max = max;
    wasmer_result_t memory_result = wasmer_memory_new(&memory, descriptor);
    printf("Memory result:  %d\n", memory_result);
    assert(memory_result == WASMER_OK);

    uint32_t len = wasmer_memory_length(memory);
    printf("Memory pages length:  %d\n", len);
    assert(len == 10);

    wasmer_result_t grow_result = wasmer_memory_grow(memory, 2);
    assert(grow_result == WASMER_OK);

    uint32_t new_len = wasmer_memory_length(memory);
    printf("Memory pages length:  %d\n", new_len);
    assert(new_len == 12);

    uint32_t bytes_len = wasmer_memory_data_length(memory);
    printf("Memory bytes length:  %d\n", bytes_len);
    assert(bytes_len == 12 * 65536);

    // Err, grow beyond max
    wasmer_result_t grow_result2 = wasmer_memory_grow(memory, 10);
    assert(grow_result2 == WASMER_ERROR);
    int error_len = wasmer_last_error_length();
    char *error_str = malloc(error_len);
    wasmer_last_error_message(error_str, error_len);
    printf("Error str: `%s`\n", error_str);
    assert(0 == strcmp(error_str, "Failed to add pages because would exceed maximum number of pages for the memory. Left: 22, Added: 15"));
    free(error_str);

    wasmer_memory_t *bad_memory = NULL;
    wasmer_limits_t bad_descriptor;
    bad_descriptor.min = 15;
    wasmer_limit_option_t max2;
    max2.has_some = true;
    max2.some = 10;
    bad_descriptor.max = max2;
    wasmer_result_t bad_memory_result = wasmer_memory_new(&bad_memory, bad_descriptor);
    printf("Bad memory result:  %d\n", bad_memory_result);
    assert(bad_memory_result == WASMER_ERROR);

    int error_len2 = wasmer_last_error_length();
    char *error_str2 = malloc(error_len2);
    wasmer_last_error_message(error_str2, error_len2);
    printf("Error str 2: `%s`\n", error_str2);
    assert(0 == strcmp(error_str2, "Unable to create because the supplied descriptor is invalid: \"Max number of memory pages is less than the minimum number of pages\""));
    free(error_str2);

    printf("Destroy memory\n");
    wasmer_memory_destroy(memory);
    return 0;
}
