#include <stdio.h>
#include "wasmer.h"

int main(int argc, const char* argv[]) {
    const char *wat_string =
        "(module\n"
        "   (type $mem_size_t (func (result i32)))\n"
        "   (type $get_at_t (func (param i32) (result i32)))\n"
        "   (type $set_at_t (func (param i32) (param i32)))\n"
        "   (memory $mem 1)\n"
        "   (func $get_at (type $get_at_t) (param $idx i32) (result i32)\n"
        "     (i32.load (local.get $idx)))\n"
        "   (func $set_at (type $set_at_t) (param $idx i32) (param $val i32)\n"
        "     (i32.store (local.get $idx) (local.get $val)))\n"
        "   (func $mem_size (type $mem_size_t) (result i32)\n"
        "     (memory.size))\n"
        "   (export \"get_at\" (func $get_at))\n"
        "   (export \"set_at\" (func $set_at))\n"
        "   (export \"mem_size\" (func $mem_size))\n"
        "   (export \"memory\" (memory $mem)))";

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

    wasm_func_t* get_at = wasm_extern_as_func(exports.data[0]);
    wasm_func_t* set_at = wasm_extern_as_func(exports.data[1]);
    wasm_func_t* mem_size = wasm_extern_as_func(exports.data[2]);
    wasm_memory_t* memory = wasm_extern_as_memory(exports.data[3]);

    printf("Querying memory size...\n");
    wasm_memory_pages_t pages = wasm_memory_size(memory);
    size_t data_size = wasm_memory_data_size(memory);
    printf("Memory size (pages): %d\n", pages);
    printf("Memory size (bytes): %d\n", (int) data_size);

    printf("Growing memory...\n");
    if (!wasm_memory_grow(memory, 2)) {
        printf("> Error growing memory!\n");

        return 1;
    }

    wasm_memory_pages_t new_pages = wasm_memory_size(memory);
    printf("New memory size (pages): %d\n", new_pages);

    int mem_addr = 0x2220;
    int val = 0xFEFEFFE;

    wasm_val_t set_at_args_val[2] = { WASM_I32_VAL(mem_addr), WASM_I32_VAL(val) };
    wasm_val_vec_t set_at_args = WASM_ARRAY_VEC(set_at_args_val);
    wasm_val_vec_t set_at_results = WASM_EMPTY_VEC;
    wasm_func_call(set_at, &set_at_args, &set_at_results);

    wasm_val_t get_at_args_val[1] = { WASM_I32_VAL(mem_addr) };
    wasm_val_vec_t get_at_args = WASM_ARRAY_VEC(get_at_args_val);
    wasm_val_t get_at_results_val[1] = { WASM_INIT_VAL };
    wasm_val_vec_t get_at_results = WASM_ARRAY_VEC(get_at_results_val);
    wasm_func_call(get_at, &get_at_args, &get_at_results);

    printf("Value at 0x%04x: %d\n", mem_addr, get_at_results_val[0].of.i32);

    wasm_extern_vec_delete(&exports);
    wasm_module_delete(module);
    wasm_instance_delete(instance);
    wasm_store_delete(store);
    wasm_engine_delete(engine);
}
