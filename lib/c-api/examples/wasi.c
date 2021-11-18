#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

#include "wasmer.h"

#define BUF_SIZE 128
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
  wasi_config_t* config = wasi_config_new("example_program");
  // TODO: error checking
  const char* js_string = "function greet(name) { return JSON.stringify('Hello, ' + name); }; print(greet('World'));";
  wasi_config_arg(config, "--eval");
  wasi_config_arg(config, js_string);
  wasi_config_capture_stdout(config);

  wasi_env_t* wasi_env = wasi_env_new(config);
  if (!wasi_env) {
    printf("> Error building WASI env!\n");
    print_wasmer_error();
    return 1;
  }

  // Instantiate.
  printf("Instantiating module...\n");
  wasm_extern_vec_t imports;
  bool get_imports_result = wasi_get_imports(store, module, wasi_env, &imports);

  if (!get_imports_result) {
    printf("> Error getting WASI imports!\n");
    print_wasmer_error();
    return 1;
  }

  own wasm_instance_t* instance =
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

  wasm_func_t* run_func = wasi_get_start_function(instance);
  if (run_func == NULL) {
    printf("> Error accessing export!\n");
    print_wasmer_error();
    return 1;
  }

  wasm_module_delete(module);
  wasm_instance_delete(instance);

  // Call.
  printf("Calling export...\n");
  printf("Evaluating \"%s\"\n", js_string);

  wasm_val_vec_t args = WASM_EMPTY_VEC;
  wasm_val_vec_t res = WASM_EMPTY_VEC;

  if (wasm_func_call(run_func, &args, &res)) {
    printf("> Error calling function!\n");
    return 1;
  }

  {
    FILE *memory_stream;
    char* stdout;
    size_t stdout_size = 0;

    memory_stream = open_memstream(&stdout, &stdout_size);

    if (NULL == memory_stream) {
      printf("> Error creating a memory stream.\n");
      return 1;
    }

    char buffer[BUF_SIZE] = { 0 };
    size_t data_read_size = BUF_SIZE;

    do {
      data_read_size = wasi_env_read_stdout(wasi_env, buffer, BUF_SIZE);

      if (data_read_size > 0) {
        stdout_size += data_read_size;
        fwrite(buffer, sizeof(char), data_read_size, memory_stream);
      }
    } while (BUF_SIZE == data_read_size);

    fclose(memory_stream);

    printf("WASI Stdout: `%.*s`\n", (int) stdout_size, stdout);
    free(stdout);
  }


  wasm_extern_vec_delete(&exports);
  wasm_extern_vec_delete(&imports);

  // Shut down.
  printf("Shutting down...\n");
  wasm_func_delete(run_func);
  wasi_env_delete(wasi_env);
  wasm_store_delete(store);
  wasm_engine_delete(engine);

  // All done.
  printf("Done.\n");
  return 0;
}
