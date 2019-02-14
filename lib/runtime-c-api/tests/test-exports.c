#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

int main()
{
    wasmer_import_object_t *import_object = wasmer_import_object_new();

    // Read the wasm file bytes
    FILE *file = fopen("sum.wasm", "r");
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

    wasmer_exports_t *exports = NULL;
    wasmer_instance_exports(instance, &exports);

    int exports_len = wasmer_exports_len(exports);
    printf("exports_len:  %d\n", exports_len);
    assert(exports_len == 1);

    wasmer_export_t *export = wasmer_exports_get(exports, 0);

    wasmer_import_export_kind kind = wasmer_export_kind(export);
    assert(kind == WASM_FUNCTION);
    wasmer_func_t *func = wasmer_export_to_func(export);

    wasmer_byte_array name_bytes = wasmer_export_name(export);
    assert(name_bytes.bytes_len == 3);
    char expected[] = {'s', 'u', 'm'};
    for(int idx = 0; idx < 3; idx++){
        printf("%c\n", name_bytes.bytes[idx]);
        assert(name_bytes.bytes[idx] == expected[idx]);
    }

//    wasmer_value_t param_one;
//    param_one.tag = WASM_I32;
//    param_one.value.I32 = 7;
//    wasmer_value_t param_two;
//    param_two.tag = WASM_I32;
//    param_two.value.I32 = 8;
//    wasmer_value_t params[] = {param_one, param_two};
//    wasmer_value_t result_one;
//    wasmer_value_t results[] = {result_one};
//
//    wasmer_result_t call_result = wasmer_func_call(func, params, 2, results, 1);
//    printf("Call result:  %d\n", call_result);
//    printf("Result: %d\n", results[0].value.I32);
//    assert(results[0].value.I32 == 15);
//    assert(call_result == WASMER_OK);


    printf("Destroy instance\n");
    wasmer_instance_destroy(instance);
    printf("Destroy import object\n");
    wasmer_import_object_destroy(import_object);
    printf("Destroy exports\n");
    wasmer_exports_destroy(exports);
    return 0;
}