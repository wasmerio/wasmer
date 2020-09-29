#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

#include "wasm.h"
#include "wasmer_wasm.h"

// Use the last_error API to retrieve error messages
void print_wasmer_error()
{
    int error_len = wasmer_last_error_length();
    printf("Error len: `%d`\n", error_len);
    char *error_str = malloc(error_len);
    wasmer_last_error_message(error_str, error_len);
    printf("Error str: `%s`\n", error_str);
}

wasm_store_t* store = NULL;

wasm_trap_t* early_exit(wasm_val_t args[], wasm_val_t results[]) {
        wasm_message_t* trap_message = NULL;
        const char* message_inner = "trapping from a host import";
        wasm_byte_vec_new_uninitialized(trap_message, strlen(message_inner));
        // TODO: should we free this data?
        return wasm_trap_new(store, trap_message);
}

int main(int argc, const char* argv[]) {
  // Initialize.
  printf("Initializing...\n");
  wasm_engine_t* engine = wasm_engine_new();
  store = wasm_store_new(engine);

  // Load binary.
  printf("Loading binary...\n");
  FILE* file = fopen("assets/call_trap.wasm", "r");
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
  own wasm_module_t* module = wasm_module_new(store, &binary);
  if (!module) {
    printf("> Error compiling module!\n");
    return 1;
  }

  wasm_byte_vec_delete(&binary);

  // Instantiate.
  printf("Instantiating module...\n");

  // TODO: fill imports

  wasm_functype_t* host_func_type = wasm_functype_new_0_0();
  wasm_func_t* host_func = wasm_func_new(store, host_func_type, (wasm_func_callback_t)early_exit);
  wasm_extern_t* host_func_as_extern = wasm_func_as_extern(host_func);
  wasm_functype_delete(host_func_type);

  wasm_extern_t* imports[] = { host_func_as_extern };

  own wasm_instance_t* instance =
    wasm_instance_new(store, module, (const wasm_extern_t *const *) imports, NULL);
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

  // TODO:
  wasm_func_t* run_func = NULL;
  if (run_func == NULL) {
    printf("> Error accessing export!\n");
    print_wasmer_error();
    return 1;
  }

  wasm_module_delete(module);
  wasm_instance_delete(instance);

  // Call.
  printf("Calling export...\n");
  if (wasm_func_call(run_func, NULL, NULL)) {
    printf("> Error calling function!\n");
    return 1;
  }

  wasm_extern_vec_delete(&exports);

  // NEEDS REVIEW:
  for(int i = 0; i < num_imports; ++i) {
     wasm_extern_delete(imports[i]);
  }
  free(imports);

  // Shut down.
  printf("Shutting down...\n");
  wasm_func_delete(run_func);
  wasm_store_delete(store);
  wasm_engine_delete(engine);

  // All done.
  printf("Done.\n");
  return 0;
}
