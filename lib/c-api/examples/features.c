#include <stdio.h>
#include "wasmer.h"

int main(int argc, const char* argv[]) {
    // This is not enabled in JavascriptCore
    #ifndef WASMER_JSC_BACKEND

    const char *wat_string =
        "(module\n"
        "  (type $swap_t (func (param i32 i64) (result i64 i32)))\n"
        "  (func $swap (type $swap_t) (param $x i32) (param $y i64) (result i64 i32)\n"
        "    (local.get $y)\n"
        "    (local.get $x))\n"
        "  (export \"swap\" (func $swap)))";

    wasm_byte_vec_t wat;
    wasm_byte_vec_new(&wat, strlen(wat_string), wat_string);
    wasm_byte_vec_t wasm_bytes;
    wat2wasm(&wat, &wasm_bytes);
    wasm_byte_vec_delete(&wat);

    printf("Creating the config and the features...\n");
    wasm_config_t* config = wasm_config_new();

    wasmer_features_t* features = wasmer_features_new();
    wasmer_features_multi_value(features, true); // enable multi-value!
    wasm_config_set_features(config, features);

    printf("Creating the store...\n");
    wasm_engine_t* engine = wasm_engine_new_with_config(config);
    wasm_store_t* store = wasm_store_new(engine);

    printf("Compiling module...\n");
    wasm_module_t* module = wasm_module_new(store, &wasm_bytes);

    if (!module) {
        printf("> Error compiling module!\n");

        return 1;
    }

    wasm_byte_vec_delete(&wasm_bytes);

    printf("Instantiating module...\n");
    wasm_extern_vec_t imports = WASM_EMPTY_VEC;
    wasm_trap_t* trap = NULL;
    wasm_instance_t* instance = wasm_instance_new(store, module, &imports,&trap);

    if (!instance) {
        printf("> Error instantiating module!\n");

        return 1;
    }

    printf("Retrieving exports...\n");
    wasm_extern_vec_t exports;
    wasm_instance_exports(instance, &exports);

    if (exports.size == 0) {
        printf("> Error accessing exports!\n");

        return 1;
    }

    printf("Executing `swap(1, 2)`...\n");
    wasm_func_t* swap = wasm_extern_as_func(exports.data[0]);

    wasm_val_t arguments[2] = { WASM_I32_VAL(1), WASM_I64_VAL(2) };
    wasm_val_t results[2] = { WASM_INIT_VAL, WASM_INIT_VAL };
    wasm_val_vec_t arguments_as_array = WASM_ARRAY_VEC(arguments);
    wasm_val_vec_t results_as_array = WASM_ARRAY_VEC(results);

    trap = wasm_func_call(swap, &arguments_as_array, &results_as_array);

    if (trap != NULL) {
        printf("> Failed to call `swap`.\n");

        return 1;
    }

    if (results[0].of.i64 != 2 || results[1].of.i32 != 1) {
        printf("> Multi-value failed.\n");

        return 1;
    }

    printf("Got `(2, 1)`!\n");

    wasm_extern_vec_delete(&exports);
    wasm_module_delete(module);
    wasm_instance_delete(instance);
    wasm_store_delete(store);
    wasm_engine_delete(engine);

    #endif //WASMER_JSC_BACKEND
}
