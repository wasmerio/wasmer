#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>

typedef struct {
    int32_t amount;
    int32_t value;
} counter_data;

typedef struct {
    uint8_t* bytes;
    long bytes_len;
} wasm_file_t;

wasm_file_t read_wasm_file(const char* file_name) {
    wasm_file_t wasm_file;

    FILE *file = fopen(file_name, "r");
    fseek(file, 0, SEEK_END);
    wasm_file.bytes_len = ftell(file);

    wasm_file.bytes = malloc(wasm_file.bytes_len);
    fseek(file, 0, SEEK_SET);
    fread(wasm_file.bytes, 1, wasm_file.bytes_len, file);
    fclose(file);

    return wasm_file;
}

void inc_counter(wasmer_instance_context_t *ctx) {
    counter_data* data = (counter_data*)wasmer_instance_context_data_get(ctx);
    data->value = data->value + data->amount;
}

void mul_counter(wasmer_instance_context_t *ctx) {
    counter_data* data = (counter_data*)wasmer_instance_context_data_get(ctx);
    data->value = data->value * data->amount;
}

int32_t get_counter(wasmer_instance_context_t *ctx) {
    counter_data* data = (counter_data*)wasmer_instance_context_data_get(ctx);
    return data->value;
}

counter_data *init_counter(int32_t value, int32_t amount) {
    counter_data* counter = malloc(sizeof(counter_data));
    counter->value = value;
    counter->amount = amount;
    return counter;
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
    // Prepare Imports
    wasmer_value_tag inc_params_sig[] = {};
    wasmer_value_tag inc_returns_sig[] = {};
    wasmer_import_func_t *inc_func = wasmer_import_func_new((void (*)(void *)) inc_counter, inc_params_sig, 0, inc_returns_sig, 0);
    wasmer_import_t inc_import = create_import("env", "inc", inc_func);

    wasmer_value_tag mul_params_sig[] = {};
    wasmer_value_tag mul_returns_sig[] = {};
    wasmer_import_func_t *mul_func = wasmer_import_func_new((void (*)(void *)) mul_counter, mul_params_sig, 0, mul_returns_sig, 0);
    wasmer_import_t mul_import = create_import("env", "mul", mul_func);

    wasmer_value_tag get_params_sig[] = {};
    wasmer_value_tag get_returns_sig[] = {WASM_I32};
    wasmer_import_func_t *get_func = wasmer_import_func_new((void (*)(void *)) get_counter, get_params_sig, 0, get_returns_sig, 1);
    wasmer_import_t get_import = create_import("env", "get", get_func);

    // Read the wasm file
    wasm_file_t wasm_file = read_wasm_file("assets/inc.wasm");

    // Compile module
		wasmer_module_t *module = NULL;
		wasmer_result_t compile_res = wasmer_compile(&module, wasm_file.bytes, wasm_file.bytes_len);
		assert(compile_res == WASMER_OK);

		// Prepare Import Object
    wasmer_import_object_t *import_object = wasmer_import_object_new();

    // First, we import `inc_counter` and `mul_counter`
    wasmer_import_t imports[] = {inc_import, mul_import};
    wasmer_result_t extend_res = wasmer_import_object_extend(import_object, imports, 2);
    assert(extend_res == WASMER_OK);

    // Now, we'll import `inc_counter` and `mul_counter`
    wasmer_import_t more_imports[] = {get_import};
    wasmer_result_t extend_res2 = wasmer_import_object_extend(import_object, more_imports, 1);
    assert(extend_res2 == WASMER_OK);

    // Same `wasmer_import_object_extend` as the first, doesn't affect anything
    wasmer_result_t extend_res3 = wasmer_import_object_extend(import_object, imports, 2);
    assert(extend_res3 == WASMER_OK);

    // Instantiate instance
    printf("Instantiating\n");
    wasmer_instance_t *instance = NULL;
    wasmer_result_t instantiate_res = wasmer_module_import_instantiate(&instance, module, import_object);
    printf("Compile result:  %d\n", instantiate_res);
    assert(instantiate_res == WASMER_OK);

    // Init counter
    counter_data *counter = init_counter(2, 5);
    wasmer_instance_context_data_set(instance, counter);

    wasmer_value_t result_one;
    wasmer_value_t params[] = {};
    wasmer_value_t results[] = {result_one};

    wasmer_result_t call1_result = wasmer_instance_call(instance, "inc_and_get", params, 0, results, 1);
    printf("Call result:  %d\n", call1_result);
    printf("Result: %d\n", results[0].value.I32);

    wasmer_result_t call2_result = wasmer_instance_call(instance, "mul_and_get", params, 0, results, 1);
    printf("Call result:  %d\n", call2_result);
    printf("Result: %d\n", results[0].value.I32);

    // Clear resources
    wasmer_import_func_destroy(inc_func);
    wasmer_import_func_destroy(mul_func);
    wasmer_import_func_destroy(get_func);
    wasmer_instance_destroy(instance);
    free(counter);
    free(wasm_file.bytes);

    return 0;
}
