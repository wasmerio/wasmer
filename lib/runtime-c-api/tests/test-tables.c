#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

int main()
{
    wasmer_table_t *table = NULL;
    wasmer_limits_t descriptor;
    descriptor.min = 10;
    descriptor.max = 10;
    wasmer_table_result_t table_result = wasmer_table_new(&table, descriptor);
    printf("Table result:  %d\n", table_result);
    assert(table_result == WASMER_TABLE_OK);

    uint32_t len = wasmer_table_length(table);
    printf("Table length:  %d\n", len);
    assert(len == 10);

    printf("Destroy table\n");
    wasmer_table_destroy(table);
    return 0;
}
