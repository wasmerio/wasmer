#include "wasmer.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef __cplusplus
extern "C" {
#endif

extern size_t WASMER_MODULE_LENGTH asm("WASMER_MODULE_LENGTH");
extern char WASMER_MODULE_DATA asm("WASMER_MODULE_DATA");

wasm_module_t* wasmer_object_module_new(wasm_store_t* store, const char* wasm_name) {
  wasm_byte_vec_t module_byte_vec = {
    .size = WASMER_MODULE_LENGTH,
    .data = (const char*)&WASMER_MODULE_DATA,
  };
  wasm_module_t* module = wasm_module_deserialize(store, &module_byte_vec);

  return module;
}

#ifdef __cplusplus
}
#endif
