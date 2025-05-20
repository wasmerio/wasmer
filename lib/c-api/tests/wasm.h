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

  char *wasmer_test_backend = getenv("WASMER_CAPI_CONFIG");
  char *wasmer_test_engine;

  printf("Using backend: %s\n", wasmer_test_backend);

  strtok_r(wasmer_test_backend, "-", &wasmer_test_engine);

  if (strcmp(wasmer_test_backend, "cranelift") == 0) {
    assert(wasmer_is_backend_available(CRANELIFT));
    wasm_config_set_backend(config, CRANELIFT);
  } else if (strcmp(wasmer_test_backend, "llvm") == 0) {
    assert(wasmer_is_backend_available(LLVM));
    wasm_config_set_backend(config, LLVM);
  } else if (strcmp(wasmer_test_backend, "singlepass") == 0) {
    assert(wasmer_is_backend_available(SINGLEPASS));
    wasm_config_set_backend(config, SINGLEPASS);
  } else if (strcmp(wasmer_test_backend, "headless") == 0) {
    assert(wasmer_is_backend_available(HEADLESS));
    wasm_config_set_backend(config, HEADLESS);
  } else if (strcmp(wasmer_test_backend, "v8") == 0) {
    assert(wasmer_is_backend_available(V8));
    wasm_config_set_backend(config, V8);
  } else if (strcmp(wasmer_test_backend, "wamr") == 0) {
    assert(wasmer_is_backend_available(WAMR));
    wasm_config_set_backend(config, WAMR);
  } else if (strcmp(wasmer_test_backend, "wasmi") == 0) {
    assert(wasmer_is_backend_available(WASMI));
    wasm_config_set_backend(config, WASMI);
  } else if (wasmer_test_backend) {
    printf("Compiler %s not recognized\n", wasmer_test_backend);
    abort();
  }

  wasm_engine_t *engine = wasm_engine_new_with_config(config);
  return engine;
}

#endif
