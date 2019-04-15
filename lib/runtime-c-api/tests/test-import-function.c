#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>

static bool print_str_called = false;
static int memory_len = 0;
static int ptr_len = 0;
static char actual_str[14] = {};
static int actual_context_data_value = 0;

typedef struct {
  int value;
} context_data;

void print_str(wasmer_instance_context_t *ctx, int32_t ptr, int32_t len)
{
    const wasmer_memory_t *memory = wasmer_instance_context_memory(ctx, 0);
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

    actual_context_data_value = ((context_data *) wasmer_instance_context_data_get(ctx))->value;
}

int main()
{
    wasmer_value_tag params_sig[] = {WASM_I32, WASM_I32};
    wasmer_value_tag returns_sig[] = {};

    printf("Creating new func\n");
    wasmer_import_func_t *func = wasmer_import_func_new((void (*)(void *)) print_str, params_sig, 2, returns_sig, 0);
    wasmer_import_t import;

    char *module_name = "env";
    wasmer_byte_array module_name_bytes;
    module_name_bytes.bytes = (const uint8_t *) module_name;
    module_name_bytes.bytes_len = strlen(module_name);
    char *import_name = "print_str";
    wasmer_byte_array import_name_bytes;
    import_name_bytes.bytes = (const uint8_t *) import_name;
    import_name_bytes.bytes_len = strlen(import_name);

    import.module_name = module_name_bytes;
    import.import_name = import_name_bytes;
    import.tag = WASM_FUNCTION;
    import.value.func = func;
    wasmer_import_t imports[] = {import};

    // Read the wasm file bytes
    FILE *file = fopen("assets/wasm_sample_app.wasm", "r");
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

    context_data* context_data = malloc(sizeof(context_data));
    int context_data_value = 42;
    context_data->value = context_data_value;
    wasmer_instance_context_data_set(instance, context_data);

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
    assert(context_data_value == actual_context_data_value);

    printf("Destroying func\n");
    wasmer_import_func_destroy(func);
    printf("Destroy instance\n");
    wasmer_instance_destroy(instance);
    free(context_data);
    return 0;
}
