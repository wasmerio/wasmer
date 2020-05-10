#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

int main()
{
    wasmer_value_t val;
    val.tag = WASM_I32;
    val.value.I32 = 7;
    wasmer_global_t *global = wasmer_global_new(val, true);

    wasmer_value_t get_val = wasmer_global_get(global);
    assert( get_val.value.I32 == 7);

    wasmer_value_t val2;
    val2.tag = WASM_I32;
    val2.value.I32 = 14;
    wasmer_global_set(global, val2);

    wasmer_value_t new_get_val = wasmer_global_get(global);
    assert( new_get_val.value.I32 == 14);

    wasmer_global_descriptor_t desc = wasmer_global_get_descriptor(global);
    assert(desc.mutable_);
    assert(desc.kind == WASM_I32);

    wasmer_global_destroy(global);
    return 0;
}
