#include <stdio.h>
#include "wasmer.h"

wasm_trap_t* host_func_callback(const wasm_val_vec_t* args, wasm_val_vec_t* results) {
    printf("Calling back...\n> ");

    wasm_val_t val = WASM_I32_VAL(42);
    wasm_val_copy(&results->data[0], &val);

    wasm_val_delete(&val);

    return NULL;
}

int main(int argc, const char* argv[]) {
    const char *wat_string =
        "(module\n"
        "  (func $host_function (import \"\" \"host_function\") (result i32))\n"
        "  (global $host_global (import \"env\" \"host_global\") i32)\n"
        "  (func $function (export \"guest_function\") (result i32) (global.get $global))\n"
        "  (global $global (export \"guest_global\") i32 (i32.const 42))\n"
        "  (table $table (export \"guest_table\") 1 1 funcref)\n"
        "  (memory $memory (export \"guest_memory\") 1))";

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

    printf("Creating the imported function...\n");
    wasm_functype_t* host_func_type = wasm_functype_new_0_1(wasm_valtype_new_i32());
    wasm_func_t* host_func = wasm_func_new(store, host_func_type, host_func_callback);
    wasm_functype_delete(host_func_type);

    printf("Creating the imported global...\n");
    wasm_globaltype_t* host_global_type = wasm_globaltype_new(wasm_valtype_new(WASM_F32), WASM_CONST);
    wasm_val_t host_global_val = WASM_I32_VAL(42);
    wasm_global_t* host_global = wasm_global_new(store, host_global_type, &host_global_val);
    wasm_globaltype_delete(host_global_type);

    wasm_extern_t* externs[] = {
        wasm_func_as_extern(host_func),
        wasm_global_as_extern(host_global)
    };

    wasm_extern_vec_t import_object = WASM_ARRAY_VEC(externs);

    printf("Instantiating module...\n");
    wasm_instance_t* instance = wasm_instance_new(store, module, &import_object, NULL);
    wasm_func_delete(host_func);
    wasm_global_delete(host_global);

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

    printf("Retrieving the exported function...\n");
    wasm_func_t* func = wasm_extern_as_func(exports.data[0]);

    if (func == NULL) {
        printf("> Failed to get the exported function!\n");

        return 1;
    }

    printf("Got the exported function: %p\n", func);

    printf("Retrieving the exported global...\n");
    wasm_global_t* global = wasm_extern_as_global(exports.data[1]);

    if (global == NULL) {
        printf("> Failed to get the exported global!\n");

        return 1;
    }

    printf("Got the exported global: %p\n", global);

    printf("Retrieving the exported table...\n");
    wasm_table_t* table = wasm_extern_as_table(exports.data[2]);

    if (table == NULL) {
        printf("> Failed to get the exported table!\n");

        return 1;
    }

    printf("Got the exported table: %p\n", table);

    printf("Retrieving the exported memory...\n");
    wasm_memory_t* memory = wasm_extern_as_memory(exports.data[3]);

    if (memory == NULL) {
        printf("> Failed to get the exported memory!\n");

        return 1;
    }

    printf("Got the exported memory: %p\n", memory);

    wasm_module_delete(module);
    wasm_extern_vec_delete(&exports);
    wasm_instance_delete(instance);
    wasm_store_delete(store);
    wasm_engine_delete(engine);
}
