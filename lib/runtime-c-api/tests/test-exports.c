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

    printf("Destroy instance\n");
    wasmer_instance_destroy(instance);
    printf("Destroy import object\n");
    wasmer_import_object_destroy(import_object);
    printf("Destroy exports\n");
    wasmer_exports_destroy(exports);
    return 0;
}