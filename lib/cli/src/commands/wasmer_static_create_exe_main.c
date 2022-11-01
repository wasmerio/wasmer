#include "wasmer.h"
#include "static_defs.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define own

// TODO: make this define templated so that the Rust code can toggle it on/off
#define WASI

#ifdef WASI_PIRITA
extern size_t VOLUMES_LENGTH asm("VOLUMES_LENGTH");
extern char VOLUMES_DATA asm("VOLUMES_DATA");
#endif

extern wasm_module_t* wasmer_object_module_new(wasm_store_t* store,const char* wasm_name) asm("wasmer_object_module_new");

static void print_wasmer_error() {
  int error_len = wasmer_last_error_length();
  printf("Error len: `%d`\n", error_len);
  char *error_str = (char *)malloc(error_len);
  wasmer_last_error_message(error_str, error_len);
  printf("%s\n", error_str);
  free(error_str);
}

#ifdef WASI
static void pass_mapdir_arg(wasi_config_t *wasi_config, char *mapdir) {
  int colon_location = strchr(mapdir, ':') - mapdir;
  if (colon_location == 0) {
    // error malformed argument
    fprintf(stderr, "Expected mapdir argument of the form alias:directory\n");
    exit(-1);
  }

  char *alias = (char *)malloc(colon_location + 1);
  memcpy(alias, mapdir, colon_location);
  alias[colon_location] = '\0';

  int dir_len = strlen(mapdir) - colon_location;
  char *dir = (char *)malloc(dir_len + 1);
  memcpy(dir, &mapdir[colon_location + 1], dir_len);
  dir[dir_len] = '\0';

  wasi_config_mapdir(wasi_config, alias, dir);
  free(alias);
  free(dir);
}

// We try to parse out `--dir` and `--mapdir` ahead of time and process those
// specially. All other arguments are passed to the guest program.
static void handle_arguments(wasi_config_t *wasi_config, int argc,
                             char *argv[]) {
  for (int i = 1; i < argc; ++i) {
    // We probably want special args like `--dir` and `--mapdir` to not be
    // passed directly
    if (strcmp(argv[i], "--dir") == 0) {
      // next arg is a preopen directory
      if ((i + 1) < argc) {
        i++;
        wasi_config_preopen_dir(wasi_config, argv[i]);
      } else {
        fprintf(stderr, "--dir expects a following argument specifying which "
                        "directory to preopen\n");
        exit(-1);
      }
    } else if (strcmp(argv[i], "--mapdir") == 0) {
      // next arg is a mapdir
      if ((i + 1) < argc) {
        i++;
        pass_mapdir_arg(wasi_config, argv[i]);
      } else {
        fprintf(stderr,
                "--mapdir expects a following argument specifying which "
                "directory to preopen in the form alias:directory\n");
        exit(-1);
      }
    } else if (strncmp(argv[i], "--dir=", strlen("--dir=")) == 0) {
      // this arg is a preopen dir
      char *dir = argv[i] + strlen("--dir=");
      wasi_config_preopen_dir(wasi_config, dir);
    } else if (strncmp(argv[i], "--mapdir=", strlen("--mapdir=")) == 0) {
      // this arg is a mapdir
      char *mapdir = argv[i] + strlen("--mapdir=");
      pass_mapdir_arg(wasi_config, mapdir);
    } else {
      // guest argument
      wasi_config_arg(wasi_config, argv[i]);
    }
  }
}
#endif

int main(int argc, char *argv[]) {
  wasm_config_t *config = wasm_config_new();
  wasm_engine_t *engine = wasm_engine_new_with_config(config);
  wasm_store_t *store = wasm_store_new(engine);

  #ifdef WASI_PIRITA
    // INSTANTIATE_MODULES
  #else
    wasm_module_t *module = wasmer_object_module_new(store, "module");
  #endif

  if (!module) {
    fprintf(stderr, "Failed to create module\n");
    print_wasmer_error();
    return -1;
  }

  // We have now finished the memory buffer book keeping and we have a valid
  // Module.

#ifdef WASI_PIRITA
  wasi_config_t *wasi_config = wasi_config_new(argv[0]);
  handle_arguments(wasi_config, argc, argv);

  wasm_byte_vec_t volume_bytes = {
    .size = VOLUMES_LENGTH,
    .data = &VOLUMES_DATA,
  };

  wasi_filesystem_t* filesystem = wasi_filesystem_init_static_memory(&volume_bytes);
  if (!filesystem) {
    printf("Error parsing filesystem from bytes\n");
    return 1;
  }

  wasm_extern_vec_t imports;
  wasi_env_t* wasi_env = wasi_env_with_filesystem(
      wasi_config,
      store,
      module,
      filesystem,
      &imports,
      "##atom-name##"
      );
  if (!wasi_env) {
    printf("Error setting filesystem\n");
    return 1;
  }
#else
  wasi_config_t *wasi_config = wasi_config_new(argv[0]);
  handle_arguments(wasi_config, argc, argv);

  wasi_env_t *wasi_env = wasi_env_new(store, wasi_config);
  if (!wasi_env) {
    fprintf(stderr, "Error building WASI env!\n");
    print_wasmer_error();
    return 1;
  }

  wasm_importtype_vec_t import_types;
  wasm_module_imports(module, &import_types);

  wasm_extern_vec_t imports;
  wasm_extern_vec_new_uninitialized(&imports, import_types.size);
  wasm_importtype_vec_delete(&import_types);

#ifdef WASI
  bool get_imports_result = wasi_get_imports(store, wasi_env, module, &imports);

  if (!get_imports_result) {
    fprintf(stderr, "Error getting WASI imports!\n");
    print_wasmer_error();

    return 1;
  }
#endif
#endif

  wasm_instance_t *instance = wasm_instance_new(store, module, &imports, NULL);

  if (!instance) {
    fprintf(stderr, "Failed to create instance\n");
    print_wasmer_error();
    return -1;
  }

#ifdef WASI
  // Read the exports.
  wasm_extern_vec_t exports;
  wasm_instance_exports(instance, &exports);
  wasm_memory_t* mem = NULL;
  for (size_t i = 0; i < exports.size; i++) {
    mem = wasm_extern_as_memory(exports.data[i]);
    if (mem) {
      break;
    }
  }

  if (!mem) {
    fprintf(stderr, "Failed to create instance: Could not find memory in exports\n");
    print_wasmer_error();
    return -1;
  }
  wasi_env_set_memory(wasi_env, mem);

  own wasm_func_t *start_function = wasi_get_start_function(instance);
  if (!start_function) {
    fprintf(stderr, "`_start` function not found\n");
    print_wasmer_error();
    return -1;
  }

  wasm_val_vec_t args = WASM_EMPTY_VEC;
  wasm_val_vec_t results = WASM_EMPTY_VEC;
  own wasm_trap_t *trap = wasm_func_call(start_function, &args, &results);
  if (trap) {
    fprintf(stderr, "Trap is not NULL: TODO:\n");
    return -1;
  }
#endif

  // TODO: handle non-WASI start (maybe with invoke?)

#ifdef WASI_PIRITA
  wasi_filesystem_delete(filesystem);
#endif
#ifdef WASI
  wasi_env_delete(wasi_env);
  wasm_extern_vec_delete(&exports);
#endif
  wasm_instance_delete(instance);
  wasm_module_delete(module);
  wasm_store_delete(store);
  wasm_engine_delete(engine);
  return 0;
}
