#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

int main()
{
    wasmer_table_t *table = NULL;
    wasmer_limits_t descriptor;
    descriptor.min = 10;
    descriptor.max = 15;
    wasmer_table_result_t table_result = wasmer_table_new(&table, descriptor);
    printf("Table result:  %d\n", table_result);
    assert(table_result == WASMER_TABLE_OK);

    uint32_t len = wasmer_table_length(table);
    printf("Table length:  %d\n", len);
    assert(len == 15);

    // wasmer_table_result_t grow_result1 = wasmer_table_grow(&table, 5);
    // assert(grow_result1 == WASMER_TABLE_OK);
    // uint32_t len_grow1 = wasmer_table_length(table);
    // printf("Table length:  %d\n", len_grow1);
    // assert(len_grow1 == 15);

    // // Try to grow beyond max
    // wasmer_table_result_t grow_result2 = wasmer_table_grow(&table, 1);
    // assert(grow_result2 == WASMER_TABLE_ERROR);
    // uint32_t len_grow2 = wasmer_table_length(table);
    // printf("Table length:  %d\n", len_grow2);
    // assert(len_grow2 == 15);

//    wasmer_table_t *table_bad = NULL;
//    wasmer_limits_t bad_descriptor;
//    bad_descriptor.min = 15;
//    bad_descriptor.max = 10;
//    wasmer_table_result_t table_bad_result = wasmer_table_new(&table_bad, bad_descriptor);
//    printf("Table result:  %d\n", table_bad_result);
//    assert(table_result == WASMER_TABLE_ERROR);

    printf("Destroy table\n");
    wasmer_table_destroy(table);
    return 0;
}
