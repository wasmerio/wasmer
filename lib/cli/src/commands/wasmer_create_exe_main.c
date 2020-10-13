#ifdef __cplusplus
extern "C" {
#endif

#include "wasmer_wasm.h"
#include "wasm.h"
#include "my_wasm.h"

#include <stdio.h>
#include <stdlib.h>

// TODO: make this define templated so that the Rust code can toggle it on/off
#define WASI

#ifdef __cplusplus
}
#endif

void print_wasmer_error()
{
  int error_len = wasmer_last_error_length();
  printf("Error len: `%d`\n", error_len);
  char* error_str = (char*) malloc(error_len);
  wasmer_last_error_message(error_str, error_len);
  printf("%s\n", error_str);
}

#ifdef WASI
int find_colon(char* string) {
  int colon_location = 0;
  for (int j = 0; j < strlen(string); ++j) {
    if (string[j] == ':') {
      colon_location = j;
      break;
    }
  }
  return colon_location;
}

void pass_mapdir_arg(wasi_config_t* wasi_config, char* mapdir) {
  int colon_location = find_colon(mapdir);
  if (colon_location == 0) {
    // error malformed argument
    fprintf(stderr, "Expected mapdir argument of the form alias:directory\n");
    exit(-1);
  }
  int dir_len = strlen(mapdir) - colon_location;
  char* alias = (char*)malloc(colon_location + 1);
  char* dir = (char*)malloc(dir_len + 1);
  int j = 0;
  for (j = 0; j < colon_location; ++j) {
    alias[j] = mapdir[j];
  }
  alias[j] = 0;
  for (j = 0; j < dir_len; ++j) {
    dir[j] = mapdir[j + colon_location + 1];
  }
  dir[j] = 0;

  wasi_config_mapdir(wasi_config, alias, dir);
  free(alias);
  free(dir);
}

// We try to parse out `--dir` and `--mapdir` ahead of time and process those
// specially. All other arguments are passed to the guest program.
void handle_arguments(wasi_config_t* wasi_config, int argc, char* argv[]) {
  for (int i = 1; i < argc; ++i) {
    // We probably want special args like `--dir` and `--mapdir` to not be passed directly
    if (strcmp(argv[i], "--dir") == 0) {
      // next arg is a preopen directory
      if ((i + 1) < argc ) {
        i++;
        wasi_config_preopen_dir(wasi_config, argv[i]);
      } else {
        fprintf(stderr, "--dir expects a following argument specifying which directory to preopen\n");
        exit(-1);
      }
    }
    else if (strcmp(argv[i], "--mapdir") == 0) {
      // next arg is a mapdir
      if ((i + 1) < argc ) {
        i++;
        pass_mapdir_arg(wasi_config, argv[i]);
      } else {
        fprintf(stderr, "--mapdir expects a following argument specifying which directory to preopen in the form alias:directory\n");
        exit(-1);
      }
    }
    else if (strncmp(argv[i], "--dir=", strlen("--dir=")) == 0 ) {
      // this arg is a preopen dir
      char* dir = argv[i] + strlen("--dir=");
      wasi_config_preopen_dir(wasi_config, dir);
    }
    else if (strncmp(argv[i], "--mapdir=", strlen("--mapdir=")) == 0 ) {
      // this arg is a mapdir
      char* mapdir = argv[i] + strlen("--mapdir=");
      pass_mapdir_arg(wasi_config, mapdir);
    }
    else {
      // guest argument
      wasi_config_arg(wasi_config, argv[i]);
    }
  }
}
#endif

int main(int argc, char* argv[]) {
  wasm_config_t* config = wasm_config_new();
  wasm_config_set_engine(config, OBJECT_FILE);
  wasm_engine_t* engine = wasm_engine_new_with_config(config);
  wasm_store_t* store = wasm_store_new(engine);
  
  wasm_module_t* module = wasmer_object_file_engine_new(store, argv[0]);
  if (! module) {
    fprintf(stderr, "Failed to create module\n");
    print_wasmer_error();
    return -1;
  }

  // We have now finished the memory buffer book keeping and we have a valid Module.
  
  #ifdef WASI
  wasi_config_t* wasi_config = wasi_config_new(argv[0]);
  handle_arguments(wasi_config, argc, argv);

  wasi_env_t* wasi_env = wasi_env_new(wasi_config);
  if (!wasi_env) {
    fprintf(stderr, "Error building WASI env!\n");
    print_wasmer_error();
    return 1;
  }
  #endif
  
  wasm_importtype_vec_t import_types;
  wasm_module_imports(module, &import_types);
  int num_imports = import_types.size;
  wasm_extern_t** imports = (wasm_extern_t**) malloc(num_imports * sizeof(wasm_extern_t*));
  wasm_importtype_vec_delete(&import_types);
  
  #ifdef WASI
  bool get_imports_result = wasi_get_imports(store, module, wasi_env, imports);
  if (!get_imports_result) {
    fprintf(stderr, "Error getting WASI imports!\n");
    print_wasmer_error();
    return 1;
  }
  #endif
  
  wasm_instance_t* instance = wasm_instance_new(store, module, (const wasm_extern_t* const*) imports, NULL);
  if (! instance) {
    fprintf(stderr, "Failed to create instance\n");
    print_wasmer_error();
    return -1;
  }

  #ifdef WASI
  wasi_env_set_instance(wasi_env, instance);
  #endif
  
  void* vmctx = wasm_instance_get_vmctx_ptr(instance);
  wasm_val_t* inout[2] = { NULL, NULL };
  
  // We're able to call our compiled function directly through a trampoline.
  wasmer_trampoline_function_call__1(vmctx, wasmer_function__1, &inout);
  
  wasm_instance_delete(instance);
  wasm_module_delete(module);
  wasm_store_delete(store);
  wasm_engine_delete(engine);
  return 0;
}
