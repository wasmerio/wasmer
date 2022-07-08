#include <stdio.h>
#include "wasmer.h"

int main(int argc, const char* argv[]) {
    const char *wat_string =
        "(module\n"
        "  (global $one (export \"one\") f32 (f32.const 1))\n"
        "  (global $some (export \"some\") (mut f32) (f32.const 0))\n"
        "  (func (export \"get_one\") (result f32) (global.get $one))\n"
        "  (func (export \"get_some\") (result f32) (global.get $some))\n"
        "  (func (export \"set_some\") (param f32) (global.set $some (local.get 0))))";

    wasm_byte_vec_t wat;
    wasm_byte_vec_new(&wat, strlen(wat_string), wat_string);
    wasm_byte_vec_t wasm_bytes;
    wat2wasm(&wat, &wasm_bytes);
    wasm_byte_vec_delete(&wat);

    printf("Creating the store...\n");
    wasm_engine_t* engine = wasm_engine_new();
    wasm_store_t* store = wasm_store_new(engine);

    printf("Compiling module...\n");
    wasm_module_t* module = wasm_module_new(store, &wasm_bytes);

    if (!module) {
        printf("> Error compiling module!\n");

        return 1;
    }

    wasm_byte_vec_delete(&wasm_bytes);

    printf("Creating imports...\n");
    wasm_extern_vec_t import_object = WASM_EMPTY_VEC;

    printf("Instantiating module...\n");
    wasm_instance_t* instance = wasm_instance_new(store, module, &import_object, NULL);

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

    wasm_global_t* one = wasm_extern_as_global(exports.data[0]);

    if (one == NULL) {
        printf("> Failed to get the `one` global!\n");

        return 1;
    }

    wasm_global_t* some = wasm_extern_as_global(exports.data[1]);

    if (some == NULL) {
        printf("> Failed to get the `some` global!\n");

        return 1;
    }

    printf("Getting globals types information...\n");
    wasm_globaltype_t* one_type = wasm_global_type(one);
    wasm_globaltype_t* some_type = wasm_global_type(some);

    wasm_mutability_t one_mutability = wasm_globaltype_mutability(one_type);
    const wasm_valtype_t* one_content = wasm_globaltype_content(one_type);
    wasm_valkind_t one_kind = wasm_valtype_kind(one_content);

    wasm_mutability_t some_mutability = wasm_globaltype_mutability(some_type);
    const wasm_valtype_t* some_content = wasm_globaltype_content(some_type);
    wasm_valkind_t some_kind = wasm_valtype_kind(some_content);

    printf("`one` type: %s %hhu\n", one_mutability == WASM_CONST ? "const" : "", one_kind);
    printf("`some` type: %s %hhu\n", some_mutability == WASM_CONST ? "const" : "", some_kind);

    printf("Getting global values...");
    wasm_val_t one_value;
    wasm_global_get(one, &one_value);
    printf("`one` value: %.1f\n", one_value.of.f32);

    wasm_val_t some_value;
    wasm_global_get(some, &some_value);
    printf("`some` value: %.1f\n", some_value.of.f32);

    printf("Setting global values...\n");
    wasm_val_t one_set_value = WASM_F32_VAL(42);
    wasm_global_set(one, &one_set_value);

    int error_length = wasmer_last_error_length();
    if (error_length > 0) {
        char *error_message = malloc(error_length);
        wasmer_last_error_message(error_message, error_length);

        printf("Attempted to set an immutable global: `%s`\n", error_message);
        free(error_message);
    }

    wasm_val_t some_set_value = WASM_F32_VAL(21);
    wasm_global_set(some, &some_set_value);
    printf("`some` value: %.1f\n", some_value.of.f32);

    wasm_globaltype_delete(one_type);
    wasm_globaltype_delete(some_type);
    wasm_module_delete(module);
    wasm_extern_vec_delete(&exports);
    wasm_instance_delete(instance);
    wasm_store_delete(store);
    wasm_engine_delete(engine);
}
