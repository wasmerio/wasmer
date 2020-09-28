#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

int main()
{
    wasmer_table_t *table = NULL;
    wasmer_limits_t descriptor;
    descriptor.min = 10;
    wasmer_limit_option_t max;
    //    max.has_some = false;
    max.has_some = true;
    max.some = 15;
    descriptor.max = max;
    wasmer_result_t table_result = wasmer_table_new(&table, descriptor);
    printf("Table result:  %d\n", table_result);
    assert(table_result == WASMER_OK);

    uint32_t len = wasmer_table_length(table);
    printf("Table length:  %d\n", len);
    assert(len == 10);

    wasmer_result_t grow_result1 = wasmer_table_grow(table, 5);
    assert(grow_result1 == WASMER_OK);
    uint32_t len_grow1 = wasmer_table_length(table);
    printf("Table length:  %d\n", len_grow1);
    assert(len_grow1 == 15);

    // Try to grow beyond max
    wasmer_result_t grow_result2 = wasmer_table_grow(table, 1);
    assert(grow_result2 == WASMER_ERROR);
    uint32_t len_grow2 = wasmer_table_length(table);
    printf("Table length:  %d\n", len_grow2);
    assert(len_grow2 == 15);

    wasmer_table_t *table_bad = NULL;
    wasmer_limits_t bad_descriptor;
    bad_descriptor.min = 15;
    wasmer_limit_option_t max2;
    max2.has_some = true;
    max2.some = 10;
    bad_descriptor.max = max2;
    wasmer_result_t table_bad_result = wasmer_table_new(&table_bad, bad_descriptor);
    printf("Table result:  %d\n", table_bad_result);
    assert(table_bad_result == WASMER_ERROR);

    printf("Destroy table\n");
    wasmer_table_destroy(table);
    return 0;
}
