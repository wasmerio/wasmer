#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

static print_str_called = false;
static memory_len = 0;
static ptr_len = 0;
static char actual_str[14] = {};

void print_str(int32_t ptr, int32_t len, wasmer_instance_context_t *ctx)
{
    wasmer_memory_t *memory = wasmer_instance_context_memory(ctx, 0);
    uint32_t mem_len = wasmer_memory_length(memory);
    uint8_t *mem_bytes = wasmer_memory_data(memory);
    for (int32_t idx = 0; idx < len; idx++)
    {
        actual_str[idx] = mem_bytes[ptr + idx];
    }
    actual_str[13] = '\0';
    printf("In print_str, memory len: %d, ptr_len: %d\n, str %s", mem_len, len, actual_str);
    print_str_called = true;
    memory_len = mem_len;
    ptr_len = len;
}

int main()
{
    wasmer_import_object_t *import_object = wasmer_import_object_new();
    wasmer_value_tag params_sig[] = {WASM_I32, WASM_I32};
    wasmer_value_tag returns_sig[] = {};
    wasmer_imports_set_import_func(import_object, "env", "print_str", print_str, params_sig, 2, returns_sig, 0);

    // Read the wasm file bytes
    FILE *file = fopen("wasm_sample_app.wasm", "r");
    fseek(file, 0, SEEK_END);
    long len = ftell(file);
    uint8_t *bytes = malloc(len);
    fseek(file, 0, SEEK_SET);
    fread(bytes, 1, len, file);
    fclose(file);

    wasmer_instance_t *instance = NULL;
    wasmer_result_t compile_result = wasmer_instantiate(&instance, bytes, len, import_object);
    printf("Compile result:  %d\n", compile_result);
    assert(compile_result == WASMER_OK);

    wasmer_value_t params[] = {};
    wasmer_value_t results[] = {};
    wasmer_result_t call_result = wasmer_instance_call(instance, "hello_wasm", params, 0, results, 0);
    printf("Call result:  %d\n", call_result);
    assert(call_result == WASMER_OK);

    assert(print_str_called);
    assert(memory_len == 17);
    assert(ptr_len == 13);
    assert(0 == strcmp(actual_str, "Hello, World!"));

    // printf("Destroy instance\n");
    // wasmer_instance_destroy(instance);
    printf("Destroy import object\n");
    wasmer_import_object_destroy(import_object);
    return 0;
}