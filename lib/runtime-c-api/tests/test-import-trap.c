#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>

static const char *trap_error_message = "Hello";

void print_str(wasmer_instance_context_t *ctx, int32_t _ptr, int32_t _len)
{
    wasmer_trap(ctx, trap_error_message);
}

int main()
{
    wasmer_value_tag params_sig[] = {WASM_I32, WASM_I32};
    wasmer_value_tag returns_sig[] = {};

    printf("Creating new func\n");
    wasmer_import_func_t *func = wasmer_import_func_new((void (*)(void *)) print_str, params_sig, 2, returns_sig, 0);

    char *module_name = "env";
    wasmer_byte_array module_name_bytes = {
        .bytes = (const uint8_t *) module_name,
        .bytes_len = strlen(module_name),
    };

    char *import_name = "print_str";
    wasmer_byte_array import_name_bytes = {
        .bytes = (const uint8_t *) import_name,
        .bytes_len = strlen(import_name),
    };

    wasmer_import_t import = {
        .module_name = module_name_bytes,
        .import_name = import_name_bytes,
        .tag = WASM_FUNCTION,
        .value.func = func,
    };

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

    wasmer_value_t params[] = {};
    wasmer_value_t results[] = {};
    wasmer_result_t call_result = wasmer_instance_call(instance, "hello_wasm", params, 0, results, 0);
    printf("Call result:  %d\n", call_result);

    assert(call_result == WASMER_ERROR);

    int error_len = wasmer_last_error_length();
    printf("Error len: `%d`\n", error_len);
    char *error_str = malloc(error_len);
    wasmer_last_error_message(error_str, error_len);
    printf("Error str: `%s`\n", error_str);

    assert(0 == strcmp(error_str, "Call error: \"Hello\""));

    printf("Destroying func\n");
    wasmer_import_func_destroy(func);
    printf("Destroy instance\n");
    wasmer_instance_destroy(instance);

    return 0;
}
