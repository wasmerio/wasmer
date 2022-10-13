#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "wasmer.h"

#define own

// Use the last_error API to retrieve error messages
void print_wasmer_error() {
  int error_len = wasmer_last_error_length();
  if (error_len > 0) {
    printf("Error len: `%d`\n", error_len);
    char *error_str = malloc(error_len);
    wasmer_last_error_message(error_str, error_len);
    printf("Error str: `%s`\n", error_str);
  }
}

void print_frame(wasm_frame_t* frame) {
  printf("> %p @ 0x%zx = %"PRIu32".0x%zx\n",
    wasm_frame_instance(frame),
    wasm_frame_module_offset(frame),
    wasm_frame_func_index(frame),
    wasm_frame_func_offset(frame)
  );
}

wasm_store_t *store = NULL;

own wasm_trap_t* early_exit(const wasm_val_vec_t* args, wasm_val_vec_t* results) {
  own wasm_message_t trap_message;
  wasm_name_new_from_string_nt(&trap_message, "trapping from a host import");
  own wasm_trap_t *trap = wasm_trap_new(store, &trap_message);
  wasm_name_delete(&trap_message);
  return trap;
}

int main(int argc, const char *argv[]) {
  // Initialize.
  printf("Initializing...\n");
  wasm_engine_t *engine = wasm_engine_new();
  store = wasm_store_new(engine);

  // Load binary.
  printf("Loading binary...\n");
  FILE *file = fopen("assets/call_trap.wasm", "rb");
  if (!file) {
    printf("> Error loading module!\n");
    return 1;
  }
  fseek(file, 0L, SEEK_END);
  size_t file_size = ftell(file);
  fseek(file, 0L, SEEK_SET);
  wasm_byte_vec_t binary;
  wasm_byte_vec_new_uninitialized(&binary, file_size);
  if (fread(binary.data, file_size, 1, file) != 1) {
    printf("> Error loading module!\n");
    return 1;
  }
  fclose(file);

  // Compile.
  printf("Compiling module...\n");
  own wasm_module_t *module = wasm_module_new(store, &binary);
  if (!module) {
    printf("> Error compiling module!\n");
    return 1;
  }

  wasm_byte_vec_delete(&binary);

  // Instantiate.
  printf("Instantiating module...\n");

  wasm_functype_t *host_func_type = wasm_functype_new_0_0();
  wasm_func_t *host_func = wasm_func_new(store, host_func_type, early_exit);

  wasm_functype_delete(host_func_type);

  wasm_extern_vec_t imports;
  wasm_extern_vec_new_uninitialized(&imports, 1);
  imports.data[0] = wasm_func_as_extern(host_func);

  own wasm_instance_t *instance =
      wasm_instance_new(store, module, &imports, NULL);

  if (!instance) {
    printf("> Error instantiating module!\n");
    print_wasmer_error();
    return 1;
  }

  // Extract export.
  printf("Extracting export...\n");
  own wasm_extern_vec_t exports;
  wasm_instance_exports(instance, &exports);
  if (exports.size == 0) {
    printf("> Error accessing exports!\n");
    return 1;
  }
  fprintf(stderr, "found %zu exports\n", exports.size);

  wasm_module_delete(module);
  wasm_instance_delete(instance);

  wasm_func_t *run_func = wasm_extern_as_func(exports.data[0]);
  if (run_func == NULL) {
    printf("> Error accessing export!\n");
    print_wasmer_error();
    return 1;
  }

  // Call.
  printf("Calling export...\n");
  wasm_val_t values[2] = { WASM_I32_VAL(1), WASM_I32_VAL(7) };
  own wasm_val_vec_t args = WASM_ARRAY_VEC(values);
  wasm_val_t result = WASM_INIT_VAL;
  own wasm_val_vec_t rets = { 1, &result };
  own wasm_trap_t *trap = wasm_func_call(run_func, &args, &rets);

  if (!trap) {
    printf("> Error calling function: expected trap!\n");
    return 1;
  }

  printf("Printing message...\n");
  own wasm_name_t message;
  wasm_trap_message(trap, &message);
  printf("> %s\n", message.data);

  printf("Printing origin...\n");
  own wasm_frame_t* frame = wasm_trap_origin(trap);
  if (frame) {
    print_frame(frame);
    wasm_frame_delete(frame);
  } else {
    printf("> Empty origin.\n");
  }

  printf("Printing trace...\n");
  own wasm_frame_vec_t trace;
  wasm_trap_trace(trap, &trace);
  if (trace.size > 0) {
    for (size_t i = 0; i < trace.size; ++i) {
      print_frame(trace.data[i]);
    }
  } else {
    printf("> Empty trace.\n");
  }

  wasm_frame_vec_delete(&trace);
  wasm_trap_delete(trap);
  wasm_name_delete(&message);

  wasm_extern_vec_delete(&exports);
  wasm_extern_vec_delete(&imports);

  // Shut down.
  printf("Shutting down...\n");
  wasm_store_delete(store);
  wasm_engine_delete(engine);

  // All done.
  printf("Done.\n");
  return 0;
}
