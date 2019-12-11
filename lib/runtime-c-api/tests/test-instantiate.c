#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>

int main()
{
    // Read the wasm file bytes
    FILE *file = fopen("assets/sum.wasm", "r");
    fseek(file, 0, SEEK_END);
    long len = ftell(file);
    uint8_t *bytes = malloc(len);
    fseek(file, 0, SEEK_SET);
    fread(bytes, 1, len, file);
    fclose(file);

    wasmer_import_t imports[] = {};
    wasmer_instance_t *instance = NULL;
    wasmer_result_t compile_result = wasmer_instantiate(&instance, bytes, len, imports, 0);
    printf("Compile result:  %d\n", compile_result);
    assert(compile_result == WASMER_OK);

    wasmer_value_t param_one;
    param_one.tag = WASM_I32;
    param_one.value.I32 = 7;
    wasmer_value_t param_two;
    param_two.tag = WASM_I32;
    param_two.value.I32 = 8;
    wasmer_value_t params[] = {param_one, param_two};

    wasmer_value_t result_one;
    wasmer_value_t results[] = {result_one};

    wasmer_result_t call_result = wasmer_instance_call(instance, "sum", params, 2, results, 1);
    printf("Call result:  %d\n", call_result);
    printf("Result: %d\n", results[0].value.I32);
    assert(results[0].value.I32 == 15);
    assert(call_result == WASMER_OK);


    wasmer_result_t call_result2 = wasmer_instance_call(instance, "sum", params, 1, results, 1);
    printf("Call result bad:  %d\n", call_result2);
    assert(call_result2 == WASMER_ERROR);

    int error_len = wasmer_last_error_length();
    printf("Error len: `%d`\n", error_len);
    char *error_str = malloc(error_len);
    int error_result = wasmer_last_error_message(error_str, error_len);
    assert(error_len == error_result);
    printf("Error str: `%s`\n", error_str);
    assert(0 == strcmp(error_str, "Call error: Parameters of type [I32] did not match signature [I32, I32] -> [I32]"));
    free(error_str);

    printf("Destroy instance\n");
    wasmer_instance_destroy(instance);
    return 0;
}
