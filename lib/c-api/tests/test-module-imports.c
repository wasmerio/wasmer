#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

int main()
{
    // Read the wasm file bytes
    FILE *file = fopen("assets/wasm_sample_app.wasm", "r");
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

    wasmer_import_descriptors_t *imports = NULL;
    wasmer_import_descriptors(module, &imports);

    int imports_len = wasmer_import_descriptors_len(imports);
    printf("imports_len:  %d\n", imports_len);
    assert(imports_len == 1);

    wasmer_import_descriptor_t *import = wasmer_import_descriptors_get(imports, 0);

    wasmer_import_export_kind kind = wasmer_import_descriptor_kind(import);
    assert(kind == WASM_FUNCTION);

    wasmer_byte_array name_bytes = wasmer_import_descriptor_name(import);
    assert(name_bytes.bytes_len == 9);
    char expected[] = {'p', 'r', 'i', 'n', 't', '_', 's', 't', 'r'};

    for(int idx = 0; idx < 9; idx++){
        printf("%c\n", name_bytes.bytes[idx]);
        assert(name_bytes.bytes[idx] == expected[idx]);
    }

    wasmer_byte_array module_name_bytes = wasmer_import_descriptor_module_name(import);
    assert(module_name_bytes.bytes_len == 3);
    char module_expected[] = {'e', 'n', 'v'};
    for(int idx = 0; idx < 3; idx++){
        printf("%c\n", module_name_bytes.bytes[idx]);
        assert(module_name_bytes.bytes[idx] == module_expected[idx]);
    }

    printf("Destroy module\n");
    wasmer_module_destroy(module);
    printf("Destroy imports\n");
    wasmer_import_descriptors_destroy(imports);
    return 0;
}
