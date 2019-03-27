#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

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

    wasmer_module_t *module = NULL;
    wasmer_result_t compile_result = wasmer_compile(&module, bytes, len);
    printf("Compile result:  %d\n", compile_result);
    assert(compile_result == WASMER_OK);

    wasmer_import_t imports[] = {};
    wasmer_instance_t *instance = NULL;
    wasmer_result_t instantiate_result = wasmer_module_instantiate(module, &instance, imports, 0);
    printf("Instantiate result:  %d\n", compile_result);
    assert(instantiate_result == WASMER_OK);

    wasmer_export_descriptors_t *exports = NULL;
    wasmer_export_descriptors(module, &exports);

    int exports_len = wasmer_export_descriptors_len(exports);
    printf("exports_len:  %d\n", exports_len);
    assert(exports_len == 1);

    wasmer_export_descriptor_t *export = wasmer_export_descriptors_get(exports, 0);

    wasmer_import_export_kind kind = wasmer_export_descriptor_kind(export);
    assert(kind == WASM_FUNCTION);

    wasmer_byte_array name_bytes = wasmer_export_descriptor_name(export);
    assert(name_bytes.bytes_len == 3);
    char expected[] = {'s', 'u', 'm'};
    for(int idx = 0; idx < 3; idx++){
        printf("%c\n", name_bytes.bytes[idx]);
        assert(name_bytes.bytes[idx] == expected[idx]);
    }

    printf("Destroy module\n");
    wasmer_module_destroy(module);
    printf("Destroy exports\n");
    wasmer_export_descriptors_destroy(exports);
    return 0;
}
