#include <stdio.h>
#include "wasmer.h"

void print_wasmer_error() {
  int error_len = wasmer_last_error_length();
  if (error_len > 0) {
    printf("Error len: `%d`\n", error_len);
    char *error_str = malloc(error_len);
    wasmer_last_error_message(error_str, error_len);
    printf("Error str: `%s`\n", error_str);
  } else {
    printf("empty error!\n");
  }
}

void print_frame(wasm_frame_t* frame) {
  printf("> %p @ 0x%zx = %zx\n",
    wasm_frame_instance(frame),
    wasm_frame_module_offset(frame),
    // wasm_frame_func_index(frame),
    wasm_frame_func_offset(frame)
  );
}

wasm_trap_t* do_nothing(const wasm_val_vec_t* args, wasm_val_vec_t* results) {
    printf("DO NOTHING!\n");
    return NULL;
}

wasm_trap_t* host_func_callback(const wasm_val_vec_t* args, wasm_val_vec_t* results) {
    printf("Calling back...\n> ");

    wasm_val_t val = WASM_I32_VAL(42);
    wasm_val_copy(&results->data[0], &val);

    wasm_val_delete(&val);

    return NULL;
}

int main(int argc, const char* argv[]) {

    const char *wat_string = "(module \n"
      "  (type (func (result i32))) \n"
      "  (import \"env\" \"func\" (func $imported_func (type 0))) \n"
      "  (func $other_func (type 0) i32.const 11) \n"
      "  (func $func (type 0) i32.const 1 call_indirect (type 0)) \n"
      "  (table 3 3 funcref) \n"
      "  (export \"func\" (func $func)) \n"
      "  (elem (i32.const 0) func $other_func $imported_func ) \n"
      ")";


    int verbosity_level = 0;
    int use_colors = 1;
    wasmer_setup_tracing(verbosity_level, use_colors);

        // "(module\n"
        // "  (type $add_one_t (func (param i32) (result i32)))\n"
        // "  (func $add_one_f (type $add_one_t) (param $value i32) (result i32)\n"
        // "    local.get $value\n"
        // "    i32.const 1\n"
        // "    i32.add)\n"
        // "  (export \"add_one\" (func $add_one_f)))";

    printf("MODULE:\n%s\n===\n\n", wat_string);

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
        print_wasmer_error();

        return 1;
    }

    wasm_byte_vec_delete(&wasm_bytes);

    printf("Creating the imported function...\n");
    // wasm_functype_t *type_void = wasm_functype_new_0_0();
    // wasm_func_t *func_do_nothing = wasm_func_new(store, type_void, do_nothing);
    wasm_functype_t* host_func_type = wasm_functype_new_0_1(wasm_valtype_new_i32());
    wasm_func_t* host_func = wasm_func_new(store, host_func_type, host_func_callback);
    wasm_functype_delete(host_func_type);

    wasm_extern_vec_t imports;
    wasm_extern_vec_new_uninitialized(&imports, 1);
	  imports.data[0] = wasm_func_as_extern(host_func);

    printf("Instantiating module...\n");
    wasm_instance_t* instance = wasm_instance_new(store, module, &imports, NULL);

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

    const wasm_func_t* add_one_func = wasm_extern_as_func(exports.data[0]);
    if (add_one_func == NULL) {
        printf("> Error accessing export!\n");

        return 1;
    }

    wasm_module_delete(module);
    wasm_instance_delete(instance);

    printf("Calling exported function...\n");
    wasm_val_t args_val[0] = {};
    wasm_val_vec_t args = WASM_ARRAY_VEC(args_val);

    wasm_val_t results_val[1] = { WASM_INIT_VAL };
    wasm_val_vec_t results = WASM_ARRAY_VEC(results_val);

    wasm_trap_t* trap = wasm_func_call(add_one_func, &args, &results);
    if (trap != NULL) {
        wasm_message_t retrieved_message;
        wasm_trap_message(trap, &retrieved_message);
        printf("> TRAP: %s", retrieved_message.data);

        printf("Printing trace...\n");
        wasm_frame_vec_t trace;
        wasm_trap_trace(trap, &trace);
        printf("\n");

        printf("ORIGIN:\n");
        wasm_frame_t* frame = wasm_trap_origin(trap);
        if (frame) {
          print_frame(frame);
          wasm_frame_delete(frame);
        } else {
          printf("> Empty origin.\n");
        }

        printf("TRACE:\n");
        if (trace.size > 0) {
          for (size_t i = 0; i < trace.size; ++i) {
            print_frame(trace.data[i]);
          }
        } else {
          printf("> empty trace");
        }

       return 1;
    }
    printf("OK\n");

    printf("Results of `add_one`: %d\n", results_val[0].of.i32);

    wasm_extern_vec_delete(&exports);
    wasm_store_delete(store);
    wasm_engine_delete(engine);
}
