#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

static print_str_called = false;

void print_str(int32_t ptr, int32_t len, wasmer_instance_context_t *ctx) {
    printf("In print_str\n");
    print_str_called = true;
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
    wasmer_compile_result_t compile_result = wasmer_instantiate(&instance, bytes, len, import_object);
    printf("Compile result:  %d\n", compile_result);
    assert(compile_result == WASMER_COMPILE_OK);

    wasmer_value_t params[] = {};
    wasmer_value_t results[] = {};
    wasmer_call_result_t call_result = wasmer_instance_call(instance, "hello_wasm", params, 0, results, 0);
    printf("Call result:  %d\n", call_result);
    assert(call_result == WASMER_CALL_OK);

    assert(print_str_called);


    printf("Destroy instance\n");
    wasmer_instance_destroy(instance);
    printf("Destroy import object\n");
    //wasmer_import_object_destroy(import_object); // TODO update instantiate and try this again
    return 0;
}