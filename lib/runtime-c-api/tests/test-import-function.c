#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>

static print_str_called = false;
static memory_len = 0;
static ptr_len = 0;
static char actual_str[14] = {};

void print_str(wasmer_instance_context_t *ctx, int32_t ptr, int32_t len)
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
    wasmer_value_tag params_sig[] = {WASM_I32, WASM_I32};
    wasmer_value_tag returns_sig[] = {};

    printf("Creating new func\n");
    wasmer_func_t *func = wasmer_func_new(print_str, params_sig, 2, returns_sig, 0);
    wasmer_import_t import;

    char *module_name = "env";
    wasmer_byte_array module_name_bytes;
    module_name_bytes.bytes = module_name;
    module_name_bytes.bytes_len = strlen(module_name);
    char *import_name = "print_str";
    wasmer_byte_array import_name_bytes;
    import_name_bytes.bytes = import_name;
    import_name_bytes.bytes_len = strlen(import_name);

    import.module_name = module_name_bytes;
    import.import_name = import_name_bytes;
    import.tag = WASM_FUNCTION;
    import.value.func = func;
    wasmer_import_t imports[] = {import};

    // Read the wasm file bytes
    FILE *file = fopen("wasm_sample_app.wasm", "r");
    fseek(file, 0, SEEK_END);
    long len = ftell(file);
    uint8_t *bytes = malloc(len);
    fseek(file, 0, SEEK_SET);
    fread(bytes, 1, len, file);
    fclose(file);

    printf("Instantiating\n");
    wasmer_instance_t *instance = NULL;
    wasmer_result_t compile_result = wasmer_instantiate(&instance, bytes, len, imports, 1);
    printf("Compile result:  %d\n", compile_result);

    assert(compile_result == WASMER_OK);

    wasmer_value_t params[] = {};
    wasmer_value_t results[] = {};
    wasmer_result_t call_result = wasmer_instance_call(instance, "hello_wasm", params, 0, results, 0);
    printf("Call result:  %d\n", call_result);

    int error_len = wasmer_last_error_length();
    printf("Error len: `%d`\n", error_len);
    char *error_str = malloc(error_len);
    wasmer_last_error_message(error_str, error_len);
    printf("Error str: `%s`\n", error_str);

    assert(call_result == WASMER_OK);

    assert(print_str_called);
    assert(memory_len == 17);
    assert(ptr_len == 13);
    assert(0 == strcmp(actual_str, "Hello, World!"));

    printf("Destroying func\n");
    wasmer_func_destroy(func);
    printf("Destroy instance\n");
    wasmer_instance_destroy(instance);
    return 0;
}