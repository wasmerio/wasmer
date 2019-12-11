#include <inttypes.h>
#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>

int main()
{
    // Read the WebAssembly bytes.
    uint8_t *wasm_bytes = NULL;
    long wasm_bytes_length = 0;

    {
        FILE *file = fopen("assets/exports.wasm", "r");
        fseek(file, 0, SEEK_END);
        wasm_bytes_length = ftell(file);
        wasm_bytes = (uint8_t *) malloc(wasm_bytes_length);
        fseek(file, 0, SEEK_SET);
        fread(wasm_bytes, 1, wasm_bytes_length, file);
        fclose(file);
    }

    wasmer_import_t imports[] = {};
    wasmer_instance_t *instance = NULL;
    wasmer_result_t compile_result = wasmer_instantiate(&instance, wasm_bytes, wasm_bytes_length, imports, 0);

    assert(compile_result == WASMER_OK);

    wasmer_exports_t *exports = NULL;
    wasmer_instance_exports(instance, &exports);

    int exports_length = wasmer_exports_len(exports);
    printf("Number of exports: %d\n", exports_length);

    {
        printf("\nCheck the `sum` exported function\n");

        wasmer_export_t *export = wasmer_exports_get(exports, 3);
        wasmer_import_export_kind export_kind = wasmer_export_kind(export);

        assert(export_kind == WASM_FUNCTION);

        const wasmer_export_func_t *exported_function = wasmer_export_to_func(export);

        wasmer_byte_array name_bytes = wasmer_export_name(export);

        assert(name_bytes.bytes_len == sizeof("sum") - 1);
        assert(memcmp(name_bytes.bytes, "sum", sizeof("sum") - 1) == 0);

        printf("Check arity\n");

        uint32_t inputs_arity;
        wasmer_export_func_params_arity(exported_function, &inputs_arity);

        uint32_t outputs_arity;
        wasmer_export_func_returns_arity(exported_function, &outputs_arity);

        assert(inputs_arity == 2);
        assert(outputs_arity == 1);

        printf("Check signature\n");

        wasmer_value_tag *input_types = (wasmer_value_tag *) calloc(inputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_params(exported_function, input_types, inputs_arity);

        assert(input_types[0] == WASM_I32);
        assert(input_types[1] == WASM_I32);

        free(input_types);

        wasmer_value_tag *output_types = (wasmer_value_tag *) calloc(outputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_returns(exported_function, output_types, outputs_arity);

        assert(output_types[0] == WASM_I32);

        free(output_types);

        printf("Call the exported function\n");

        wasmer_value_t input_0;
        input_0.tag = WASM_I32;
        input_0.value.I32 = 7;

        wasmer_value_t input_1;
        input_1.tag = WASM_I32;
        input_1.value.I32 = 8;

        wasmer_value_t inputs[] = {input_0, input_1};

        wasmer_value_t output_0;
        wasmer_value_t outputs[] = {output_0};

        wasmer_result_t call_result = wasmer_export_func_call(exported_function, inputs, inputs_arity, outputs, outputs_arity);

        printf("Call result:  %d\n", call_result);
        printf("Result: %d\n", outputs[0].value.I32);

        assert(outputs[0].value.I32 == 15);
        assert(call_result == WASMER_OK);
    }

    {
        printf("\nCheck the `arity_0` exported function\n");

        wasmer_export_t *export = wasmer_exports_get(exports, 4);
        wasmer_import_export_kind export_kind = wasmer_export_kind(export);

        assert(export_kind == WASM_FUNCTION);

        const wasmer_export_func_t *exported_function = wasmer_export_to_func(export);
        wasmer_byte_array name_bytes = wasmer_export_name(export);

        assert(name_bytes.bytes_len == sizeof("arity_0") - 1);
        assert(memcmp(name_bytes.bytes, "arity_0", sizeof("arity_0") - 1) == 0);

        uint32_t inputs_arity;
        wasmer_export_func_params_arity(exported_function, &inputs_arity);

        uint32_t outputs_arity;
        wasmer_export_func_returns_arity(exported_function, &outputs_arity);

        assert(inputs_arity == 0);
        assert(outputs_arity == 1);

        wasmer_value_tag *output_types = (wasmer_value_tag *) calloc(outputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_returns(exported_function, output_types, outputs_arity);

        assert(output_types[0] == WASM_I32);

        free(output_types);

        wasmer_value_t inputs[] = {};

        wasmer_value_t output_0;
        wasmer_value_t outputs[] = {output_0};

        wasmer_result_t call_result = wasmer_export_func_call(exported_function, inputs, inputs_arity, outputs, outputs_arity);

        printf("Result: %d\n", outputs[0].value.I32);

        assert(outputs[0].value.I32 == 42);
        assert(call_result == WASMER_OK);
    }

    {
        printf("\nCheck the `i32_i32` exported function\n");

        wasmer_export_t *export = wasmer_exports_get(exports, 5);
        wasmer_import_export_kind export_kind = wasmer_export_kind(export);

        assert(export_kind == WASM_FUNCTION);

        const wasmer_export_func_t *exported_function = wasmer_export_to_func(export);
        wasmer_byte_array name_bytes = wasmer_export_name(export);

        assert(name_bytes.bytes_len == sizeof("i32_i32") - 1);
        assert(memcmp(name_bytes.bytes, "i32_i32", sizeof("i32_i32") - 1) == 0);

        uint32_t inputs_arity;
        wasmer_export_func_params_arity(exported_function, &inputs_arity);

        uint32_t outputs_arity;
        wasmer_export_func_returns_arity(exported_function, &outputs_arity);

        assert(inputs_arity == 1);
        assert(outputs_arity == 1);

        wasmer_value_tag *input_types = (wasmer_value_tag *) calloc(inputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_params(exported_function, input_types, inputs_arity);

        assert(input_types[0] == WASM_I32);

        free(input_types);

        wasmer_value_tag *output_types = (wasmer_value_tag *) calloc(outputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_returns(exported_function, output_types, outputs_arity);

        assert(output_types[0] == WASM_I32);

        free(output_types);

        wasmer_value_t input_0;
        input_0.tag = WASM_I32;
        input_0.value.I32 = 7;
        wasmer_value_t inputs[] = {input_0};

        wasmer_value_t output_0;
        wasmer_value_t outputs[] = {output_0};

        wasmer_result_t call_result = wasmer_export_func_call(exported_function, inputs, inputs_arity, outputs, outputs_arity);

        printf("Result: %d\n", outputs[0].value.I32);

        assert(outputs[0].value.I32 == 7);
        assert(call_result == WASMER_OK);
    }

    {
        printf("\nCheck the `i64_i64` exported function\n");

        wasmer_export_t *export = wasmer_exports_get(exports, 6);
        wasmer_import_export_kind export_kind = wasmer_export_kind(export);

        assert(export_kind == WASM_FUNCTION);

        const wasmer_export_func_t *exported_function = wasmer_export_to_func(export);
        wasmer_byte_array name_bytes = wasmer_export_name(export);

        assert(name_bytes.bytes_len == sizeof("i64_i64") - 1);
        assert(memcmp(name_bytes.bytes, "i64_i64", sizeof("i64_i64") - 1) == 0);

        uint32_t inputs_arity;
        wasmer_export_func_params_arity(exported_function, &inputs_arity);

        uint32_t outputs_arity;
        wasmer_export_func_returns_arity(exported_function, &outputs_arity);

        assert(inputs_arity == 1);
        assert(outputs_arity == 1);

        wasmer_value_tag *input_types = (wasmer_value_tag *) calloc(inputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_params(exported_function, input_types, inputs_arity);

        assert(input_types[0] == WASM_I64);

        free(input_types);

        wasmer_value_tag *output_types = (wasmer_value_tag *) calloc(outputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_returns(exported_function, output_types, outputs_arity);

        assert(output_types[0] == WASM_I64);

        free(output_types);

        wasmer_value_t input_0;
        input_0.tag = WASM_I64;
        input_0.value.I64 = 7;
        wasmer_value_t inputs[] = {input_0};

        wasmer_value_t output_0;
        wasmer_value_t outputs[] = {output_0};

        wasmer_result_t call_result = wasmer_export_func_call(exported_function, inputs, inputs_arity, outputs, outputs_arity);

        printf("Result: %" PRId64 "\n", outputs[0].value.I64);

        assert(outputs[0].value.I64 == 7);
        assert(call_result == WASMER_OK);
    }

    {
        printf("\nCheck the `f32_f32` exported function\n");

        wasmer_export_t *export = wasmer_exports_get(exports, 7);
        wasmer_import_export_kind export_kind = wasmer_export_kind(export);

        assert(export_kind == WASM_FUNCTION);

        const wasmer_export_func_t *exported_function = wasmer_export_to_func(export);
        wasmer_byte_array name_bytes = wasmer_export_name(export);

        assert(name_bytes.bytes_len == sizeof("f32_f32") - 1);
        assert(memcmp(name_bytes.bytes, "f32_f32", sizeof("f32_f32") - 1) == 0);

        uint32_t inputs_arity;
        wasmer_export_func_params_arity(exported_function, &inputs_arity);

        uint32_t outputs_arity;
        wasmer_export_func_returns_arity(exported_function, &outputs_arity);

        assert(inputs_arity == 1);
        assert(outputs_arity == 1);

        wasmer_value_tag *input_types = (wasmer_value_tag *) calloc(inputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_params(exported_function, input_types, inputs_arity);

        assert(input_types[0] == WASM_F32);

        free(input_types);

        wasmer_value_tag *output_types = (wasmer_value_tag *) calloc(outputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_returns(exported_function, output_types, outputs_arity);

        assert(output_types[0] == WASM_F32);

        free(output_types);

        wasmer_value_t input_0;
        input_0.tag = WASM_F32;
        input_0.value.F32 = 7.42;
        wasmer_value_t inputs[] = {input_0};

        wasmer_value_t output_0;
        wasmer_value_t outputs[] = {output_0};

        wasmer_result_t call_result = wasmer_export_func_call(exported_function, inputs, inputs_arity, outputs, outputs_arity);

        printf("Result: %f\n", outputs[0].value.F32);

        assert(call_result == WASMER_OK);
    }

    {
        printf("\nCheck the `f64_f64` exported function\n");

        wasmer_export_t *export = wasmer_exports_get(exports, 8);
        wasmer_import_export_kind export_kind = wasmer_export_kind(export);

        assert(export_kind == WASM_FUNCTION);

        const wasmer_export_func_t *exported_function = wasmer_export_to_func(export);
        wasmer_byte_array name_bytes = wasmer_export_name(export);

        assert(name_bytes.bytes_len == sizeof("f64_f64") - 1);
        assert(memcmp(name_bytes.bytes, "f64_f64", sizeof("f64_f64") - 1) == 0);

        uint32_t inputs_arity;
        wasmer_export_func_params_arity(exported_function, &inputs_arity);

        uint32_t outputs_arity;
        wasmer_export_func_returns_arity(exported_function, &outputs_arity);

        assert(inputs_arity == 1);
        assert(outputs_arity == 1);

        wasmer_value_tag *input_types = (wasmer_value_tag *) calloc(inputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_params(exported_function, input_types, inputs_arity);

        assert(input_types[0] == WASM_F64);

        free(input_types);

        wasmer_value_tag *output_types = (wasmer_value_tag *) calloc(outputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_returns(exported_function, output_types, outputs_arity);

        assert(output_types[0] == WASM_F64);

        free(output_types);

        wasmer_value_t input_0;
        input_0.tag = WASM_F64;
        input_0.value.F64 = 7.42;
        wasmer_value_t inputs[] = {input_0};

        wasmer_value_t output_0;
        wasmer_value_t outputs[] = {output_0};

        wasmer_result_t call_result = wasmer_export_func_call(exported_function, inputs, inputs_arity, outputs, outputs_arity);

        printf("Result: %f\n", outputs[0].value.F64);

        assert(call_result == WASMER_OK);
    }

    {
        printf("\nCheck the `string` exported function\n");

        wasmer_export_t *export = wasmer_exports_get(exports, 9);
        wasmer_import_export_kind export_kind = wasmer_export_kind(export);

        assert(export_kind == WASM_FUNCTION);

        const wasmer_export_func_t *exported_function = wasmer_export_to_func(export);
        wasmer_byte_array name_bytes = wasmer_export_name(export);

        assert(name_bytes.bytes_len == sizeof("string") - 1);
        assert(memcmp(name_bytes.bytes, "string", sizeof("string") - 1) == 0);

        uint32_t inputs_arity;
        wasmer_export_func_params_arity(exported_function, &inputs_arity);

        uint32_t outputs_arity;
        wasmer_export_func_returns_arity(exported_function, &outputs_arity);

        assert(inputs_arity == 0);
        assert(outputs_arity == 1);

        wasmer_value_tag *output_types = (wasmer_value_tag *) calloc(outputs_arity, sizeof(wasmer_value_tag));
        wasmer_export_func_returns(exported_function, output_types, outputs_arity);

        assert(output_types[0] == WASM_I32);

        free(output_types);

        wasmer_value_t inputs[] = {};

        wasmer_value_t output_0;
        wasmer_value_t outputs[] = {output_0};

        wasmer_result_t call_result = wasmer_export_func_call(exported_function, inputs, inputs_arity, outputs, outputs_arity);

        printf("Result: %d\n", outputs[0].value.I32);

        assert(outputs[0].value.I32 == 1048576);
        assert(call_result == WASMER_OK);
    }

    {
        printf("\nCheck the `void` exported function\n");

        wasmer_export_t *export = wasmer_exports_get(exports, 10);
        wasmer_import_export_kind export_kind = wasmer_export_kind(export);

        assert(export_kind == WASM_FUNCTION);

        const wasmer_export_func_t *exported_function = wasmer_export_to_func(export);
        wasmer_byte_array name_bytes = wasmer_export_name(export);

        assert(name_bytes.bytes_len == sizeof("void") - 1);
        assert(memcmp(name_bytes.bytes, "void", sizeof("void") - 1) == 0);

        uint32_t inputs_arity;
        wasmer_export_func_params_arity(exported_function, &inputs_arity);

        uint32_t outputs_arity;
        wasmer_export_func_returns_arity(exported_function, &outputs_arity);

        assert(inputs_arity == 0);
        assert(outputs_arity == 0);

        wasmer_value_t inputs[] = {};
        wasmer_value_t outputs[] = {};

        {
            wasmer_result_t call_result = wasmer_export_func_call(exported_function, inputs, inputs_arity, outputs, outputs_arity);
            assert(call_result == WASMER_OK);
        }

        {
            wasmer_result_t call_result = wasmer_export_func_call(exported_function, NULL, inputs_arity, NULL, outputs_arity);
            assert(call_result == WASMER_OK);
        }
    }

    printf("\nDestroy instance\n");

    wasmer_instance_destroy(instance);

    printf("Destroy exports\n");

    wasmer_exports_destroy(exports);

    return 0;
}
