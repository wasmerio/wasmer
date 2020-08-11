#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

//#include "wasm.h"
#include "wasmer_wasm.h"

#define own

// Use the last_error API to retrieve error messages
void print_wasmer_error()
{
    int error_len = wasmer_last_error_length();
    printf("Error len: `%d`\n", error_len);
    char *error_str = malloc(error_len);
    wasmer_last_error_message(error_str, error_len);
    printf("Error str: `%s`\n", error_str);
}


int main(int argc, const char* argv[]) {
  // Initialize.
  printf("Initializing...\n");
  wasm_engine_t* engine = wasm_engine_new();
  wasm_store_t* store = wasm_store_new(engine);

  // Load binary.
  printf("Loading binary...\n");
  FILE* file = fopen("assets/qjs.wasm", "r");
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

  printf("Setting up WASI...\n");
  wasi_file_handle_t* stdout_capturer = wasi_output_capturing_file_new();

  wasi_state_builder_t* wsb = wasi_state_builder_new("example_program");
  // TODO: error checking
  const char* js_string = "function greet(name) { return JSON.stringify('Hello, ' + name); }; print(greet('World'));";
  wasi_state_builder_arg(wsb, "--eval");
  wasi_state_builder_arg(wsb, js_string);
  wasi_state_builder_set_stdout(wsb, stdout_capturer);

  wasi_state_t* wasi_state = wasi_state_builder_build(wsb);
  if (!wasi_state) {
    printf("> Error building WASI state!\n");
    return 1;
  }
  wasi_env_t* wasi_env = wasi_env_new(wasi_state);
  if (!wasi_env) {
    printf("> Error building WASI env!\n");
    print_wasmer_error();
    return 1;
  }
  wasi_version_t version = wasi_get_wasi_version(module);

  // Instantiate.
  printf("Instantiating module...\n");
  const wasm_extern_t* const* imports = wasi_get_imports(store, module, wasi_env, version);
  if (!imports) {
    printf("> Error getting WASI imports!\n");
    print_wasmer_error();
    return 1;
  }
  own wasm_instance_t* instance =
    wasm_instance_new(store, module, imports, NULL);
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

  printf("Getting memory...\n");
  const wasm_memory_t* memory = wasm_extern_as_memory(exports.data[0]);
  if (! memory) {
    printf("Could not get memory!\n");
    return 1;
  }
  wasi_env_set_memory(wasi_env, memory);
  const wasm_func_t* run_func = wasm_extern_as_func(exports.data[1]);
  if (run_func == NULL) {
    printf("> Error accessing export!\n");
    return 1;
  }

  wasm_module_delete(module);
  wasm_instance_delete(instance);

  // Call.
  printf("Calling export...\n");
  printf("Evaluating \"%s\"\n", js_string);
  if (wasm_func_call(run_func, NULL, NULL)) {
    printf("> Error calling function!\n");
    return 1;
  }

  const int BUF_SIZE = 128;
  char buffer[BUF_SIZE] = { };
  wasi_state_t* wasi_state_ref = wasi_env_borrow_state(wasi_env);
  wasi_file_handle_t* stdout_handle = wasi_state_get_stdout(wasi_state_ref);
  if (!stdout_handle) {
    printf("> Error getting stdout!\n");
    print_wasmer_error();
    return 1;
  }
  size_t result = BUF_SIZE;
  for (size_t i = 0;
       // TODO: this code is too clever, make the control flow more obvious here
       result == BUF_SIZE &&
               (result = wasi_output_capturing_file_read(stdout_handle, buffer, BUF_SIZE, i * BUF_SIZE));
       ++i) {
     printf("%.*s", BUF_SIZE, buffer);
  }
  printf("\n");

  wasm_extern_vec_delete(&exports);

  // Shut down.
  printf("Shutting down...\n");
  wasi_env_delete(wasi_env);
  wasm_store_delete(store);
  wasm_engine_delete(engine);

  // All done.
  printf("Done.\n");
  return 0;
}
