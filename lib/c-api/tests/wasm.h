// This header file is used only for test purposes! It is used by unit
// test inside the `src/` directory for the moment.

#ifndef TEST_WASM
#define TEST_WASM

#include "../wasm.h"
#include "../wasmer.h"
#include <stdio.h>
#include <string.h>

#if defined(_WIN32) || defined(_WIN64)
#define strtok_r strtok_s
#endif

wasm_engine_t *wasm_engine_new() {
  wasm_config_t *config = wasm_config_new();

  char *wasmer_test_compiler = getenv("WASMER_CAPI_CONFIG");
  char *wasmer_test_engine;

  strtok_r(wasmer_test_compiler, "-", &wasmer_test_engine);
  printf("Using compiler: %s, engine: %s\n", wasmer_test_compiler,
         wasmer_test_engine);
  if (strcmp(wasmer_test_compiler, "cranelift") == 0) {
    assert(wasmer_is_compiler_available(CRANELIFT));
    wasm_config_set_compiler(config, CRANELIFT);
  } else if (strcmp(wasmer_test_compiler, "llvm") == 0) {
    assert(wasmer_is_compiler_available(LLVM));
    wasm_config_set_compiler(config, LLVM);
  } else if (strcmp(wasmer_test_compiler, "singlepass") == 0) {
    assert(wasmer_is_compiler_available(SINGLEPASS));
    wasm_config_set_compiler(config, SINGLEPASS);
  } else if (wasmer_test_compiler) {
    printf("Compiler %s not recognized\n", wasmer_test_compiler);
    abort();
  }
  if (strcmp(wasmer_test_engine, "universal") == 0) {
    assert(wasmer_is_engine_available(UNIVERSAL));
    wasm_config_set_engine(config, UNIVERSAL);
  } else if (wasmer_test_engine) {
    printf("Engine %s not recognized\n", wasmer_test_engine);
    abort();
  }

  wasm_engine_t *engine = wasm_engine_new_with_config(config);
  return engine;
}

#endif
