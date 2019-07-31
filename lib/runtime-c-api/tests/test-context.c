#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>

typedef struct {
  int32_t amount;
  int32_t value;
} counter_data;

void inc_counter(wasmer_instance_context_t *ctx) {
  counter_data* data = (counter_data*)wasmer_instance_context_data_get(ctx);
  data->value = data->value + data->amount;
}

int32_t get_counter(wasmer_instance_context_t *ctx) {
  counter_data* data = (counter_data*)wasmer_instance_context_data_get(ctx);
  return data->value;
}

wasmer_import_t create_import(char* module_name, char* import_name, wasmer_import_func_t *func) {
    wasmer_import_t import;
    wasmer_byte_array module_name_bytes;
    wasmer_byte_array import_name_bytes;

    module_name_bytes.bytes = (const uint8_t *) module_name;
    module_name_bytes.bytes_len = strlen(module_name);

    import_name_bytes.bytes = (const uint8_t *) import_name;
    import_name_bytes.bytes_len = strlen(import_name);

    import.module_name = module_name_bytes;
    import.import_name = import_name_bytes;

    import.tag = WASM_FUNCTION;
    import.value.func = func;

    return import;
}

int main()
{
    // Imports
    wasmer_value_tag inc_params_sig[] = {};
    wasmer_value_tag inc_returns_sig[] = {};
    wasmer_import_func_t *inc_func = wasmer_import_func_new((void (*)(void *)) inc_counter, inc_params_sig, 0, inc_returns_sig, 0);
    wasmer_import_t inc_import = create_import("env", "inc", inc_func);

    wasmer_value_tag get_params_sig[] = {};
    wasmer_value_tag get_returns_sig[] = {WASM_I32};
    wasmer_import_func_t *get_func = wasmer_import_func_new((void (*)(void *)) get_counter, get_params_sig, 0, get_returns_sig, 1);
    wasmer_import_t get_import = create_import("env", "get", get_func);

    wasmer_import_t imports[] = {inc_import, get_import};

    // Read the wasm file bytes
    FILE *file = fopen("assets/inc.wasm", "r");
    fseek(file, 0, SEEK_END);
    long len = ftell(file);
    uint8_t *bytes = malloc(len);
    fseek(file, 0, SEEK_SET);
    fread(bytes, 1, len, file);
    fclose(file);

    printf("Instantiating\n");
    wasmer_instance_t *instance = NULL;
    wasmer_result_t compile_result = wasmer_instantiate(&instance, bytes, len, imports, 2);
    printf("Compile result:  %d\n", compile_result);

    counter_data* counter = malloc(sizeof(counter_data));
    counter->value = 0;
    counter->amount = 5;
    wasmer_instance_context_data_set(instance, counter);

    wasmer_value_t result_one;
    wasmer_value_t params[] = {};
    wasmer_value_t results[] = {result_one};

    wasmer_result_t call1_result = wasmer_instance_call(instance, "inc_and_get", params, 0, results, 1);
    printf("Call result:  %d\n", call1_result);
    printf("Result: %d\n", results[0].value.I32);
    assert(results[0].value.I32 == 5);
    assert(call1_result == WASMER_OK);

    wasmer_result_t call2_result = wasmer_instance_call(instance, "inc_and_get", params, 0, results, 1);
    printf("Call result:  %d\n", call2_result);
    printf("Result: %d\n", results[0].value.I32);
    assert(results[0].value.I32 == 10);
    assert(call2_result == WASMER_OK);

    return 0;
}
