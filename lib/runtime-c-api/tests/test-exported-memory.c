#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>

int main()
{
    // Read the wasm file bytes
    FILE *file = fopen("assets/return_hello.wasm", "r");
    fseek(file, 0, SEEK_END);
    long len = ftell(file);
    uint8_t *bytes = malloc(len);
    fseek(file, 0, SEEK_SET);
    fread(bytes, 1, len, file);
    fclose(file);

    // Instantiate the module.
    wasmer_import_t imports[] = {};
    wasmer_instance_t *instance = NULL;
    wasmer_result_t compile_result = wasmer_instantiate(&instance, bytes, len, imports, 0);
    printf("Compile result: %d\n", compile_result);
    assert(compile_result == WASMER_OK);

    // Call the `return_hello` function.
    wasmer_value_t params[] = {};
    wasmer_value_t result;
    wasmer_value_t results[] = {result};

    wasmer_result_t call_result = wasmer_instance_call(instance, "return_hello", params, 0, results, 1);
    printf("Call result: %d\n", call_result);
    printf("Result: %d\n", results[0].value.I32);
    assert(call_result == WASMER_OK);
    assert(results[0].value.I32 == 1048576);

    // Get all exports.
    wasmer_exports_t *exports = NULL;
    wasmer_instance_exports(instance, &exports);

    int export_length = wasmer_exports_len(exports);
    printf("exports_length: %d\n", export_length);
    assert(export_length == 5);

    // Get the `memory` export.
    wasmer_export_t *export = wasmer_exports_get(exports, 1);
    wasmer_import_export_kind kind = wasmer_export_kind(export);
    assert(kind == WASM_MEMORY);

    wasmer_byte_array export_name = wasmer_export_name(export);
    printf("export_name: `%.*s`\n", export_name.bytes_len, export_name.bytes);

    // Cast the export into a memory.
    wasmer_memory_t *memory;
    wasmer_result_t export_to_memory_result = wasmer_export_to_memory(export, &memory);
    printf("Export to memory result: %d\n", export_to_memory_result);
    printf("Memory pointer: %p\n", memory);
    assert(export_to_memory_result == WASMER_OK);

    uint32_t memory_length = wasmer_memory_length(memory);
    assert(memory_length == 17);

    // Read the data from the memory.
    uint8_t *memory_data = wasmer_memory_data(memory);
    uint8_t *returned_string = memory_data + results[0].value.I32;

    printf("Returned string from Wasm: %s\n", returned_string);
    assert(strcmp("Hello, World!", (const char *) returned_string) == 0);

    printf("Destroy instance\n");
    wasmer_instance_destroy(instance);

    return 0;
}
