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
    wasmer_compile_result_t compile_result = wasmer_instantiate(&instance, bytes, len, import_object);
    printf("Compile result:  %d\n", compile_result);
    assert(compile_result == WASMER_COMPILE_OK);

    wasmer_call_result_t call_result = wasmer_instance_call(instance, "sum");
    printf("Call result:  %d\n", call_result);
    assert(call_result == WASMER_CALL_OK);

    printf("Destroy instance\n");
    //wasmer_instance_destroy(instance); // error here
    printf("Destroy import object\n");
    //wasmer_import_object_destroy(import_object); // error here
    return 0;
}