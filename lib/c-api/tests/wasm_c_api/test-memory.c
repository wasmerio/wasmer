#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "wasmer_wasm.h"

#define own

// Use the last_error API to retrieve error messages
own char* get_wasmer_error() {
  int error_len = wasmer_last_error_length();
  printf("Error len: `%d`\n", error_len);
  char *error_str = malloc(error_len);
  wasmer_last_error_message(error_str, error_len);
  return error_str;
}

int main(int argc, const char *argv[]) {
  printf("Initializing...\n");
  own wasm_engine_t* engine = wasm_engine_new();
  own wasm_store_t* store = wasm_store_new(engine);

  wasm_limits_t limits1 = {
    .min = 0,
    .max = wasm_limits_max_default,
  };
  own wasm_memorytype_t* memtype1 = wasm_memorytype_new(&limits1);
  own wasm_memory_t* memory1 = wasm_memory_new(store, memtype1);
  assert(memory1 == NULL);
  char* error = get_wasmer_error();
  printf("Found error string: %s\n", error);
  assert(0 == strcmp("The maximum requested memory (4294967295 pages) is greater than the maximum allowed memory (65536 pages)", error));
  free(error);

  wasm_memorytype_delete(memtype1);

  wasm_limits_t limits2 = {
    .min = 15,
    .max = 25,
  };
  own wasm_memorytype_t* memtype2 = wasm_memorytype_new(&limits2);
  own wasm_memory_t* memory2 = wasm_memory_new(store, memtype2);
  assert(memory2 != NULL);

  wasm_memorytype_delete(memtype2);
  wasm_memory_delete(memory2);

  printf("Shutting down...\n");
  wasm_store_delete(store);
  wasm_engine_delete(engine);

  printf("Done.\n");
  return 0;
}
