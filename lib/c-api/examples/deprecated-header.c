#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// This is now deprecated, but it should work regardless
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

  // =====================
  wasm_limits_t limits1 = {
    .min = 0,
    .max = 0x7FFFFFFF,
  };
  own wasm_memorytype_t* memtype1 = wasm_memorytype_new(&limits1);
  own wasm_memory_t* memory1 = wasm_memory_new(store, memtype1);
  assert(memory1 == NULL);
  char* error = get_wasmer_error();
  printf("Found error string: %s\n", error);
  // We can't validate the exact error message because it's not universal for the engines
  // assert(0 == strcmp("The maximum requested memory (2147483647 pages) is greater than the maximum allowed memory (65536 pages)", error));
  free(error);

  wasm_memorytype_delete(memtype1);

  // =====================
  wasm_limits_t limits2 = {
    .min = 15,
    .max = 25,
  };
  own wasm_memorytype_t* memtype2 = wasm_memorytype_new(&limits2);
  own wasm_memory_t* memory2 = wasm_memory_new(store, memtype2);
  assert(memory2 != NULL);

  wasm_memorytype_delete(memtype2);
  wasm_memory_delete(memory2);

  // =====================
  wasm_limits_t limits3 = {
    .min = 15,
    .max = wasm_limits_max_default,
  };
  own wasm_memorytype_t* memtype3 = wasm_memorytype_new(&limits3);
  own wasm_memory_t* memory3 = wasm_memory_new(store, memtype3);
  assert(memory3 != NULL);
  int size = wasm_memory_size(memory3);
  printf("memory size: %d\n", size);

  wasm_memorytype_delete(memtype3);
  wasm_memory_delete(memory3);

  // =====================
  wasm_limits_t limits4 = {
    .min = 0x7FFFFFFF,
    .max = 0x7FFFFFFF,
  };
  own wasm_memorytype_t* memtype4 = wasm_memorytype_new(&limits4);
  own wasm_memory_t* memory4 = wasm_memory_new(store, memtype4);
  assert(memory4 == NULL);
  error = get_wasmer_error();
  printf("Found error string: %s\n", error);
  // We can't validate the exact error message because it's not universal for the engines
  // assert(0 == strcmp("The minimum requested (2147483647 pages) memory is greater than the maximum allowed memory (65536 pages)", error));
  free(error);

  wasm_memorytype_delete(memtype4);

  // =====================
  wasm_limits_t limits5 = {
    .min = 0x7FFFFFFF,
    .max = 0x0FFFFFFF,
  };
  own wasm_memorytype_t* memtype5 = wasm_memorytype_new(&limits5);
  own wasm_memory_t* memory5 = wasm_memory_new(store, memtype5);
  assert(memory5 == NULL);
  error = get_wasmer_error();
  printf("Found error string: %s\n", error);
  // We can't validate the exact error message because it's not universal for the engines
  // assert(0 == strcmp("The minimum requested (2147483647 pages) memory is greater than the maximum allowed memory (65536 pages)", error));
  free(error);

  wasm_memorytype_delete(memtype5);

  // =====================
  wasm_limits_t limits6 = {
    .min = 15,
    .max = 10,
  };
  own wasm_memorytype_t* memtype6 = wasm_memorytype_new(&limits6);
  own wasm_memory_t* memory6 = wasm_memory_new(store, memtype6);
  assert(memory6 == NULL);
  error = get_wasmer_error();
  printf("Found error string: %s\n", error);
  // We can't validate the exact error message because it's not universal for the engines
  // assert(0 == strcmp("The memory is invalid because the maximum (10 pages) is less than the minimum (15 pages)", error));
  free(error);

  wasm_memorytype_delete(memtype6);

  // =====================
  wasm_limits_t limits7 = {
    .min = 0x7FFFFFFF,
    .max = 10,
  };
  own wasm_memorytype_t* memtype7 = wasm_memorytype_new(&limits7);
  own wasm_memory_t* memory7 = wasm_memory_new(store, memtype7);
  assert(memory7 == NULL);
  error = get_wasmer_error();
  printf("Found error string: %s\n", error);
  // We can't validate the exact error message because it's not universal for the engines
  // assert(0 == strcmp("The minimum requested (2147483647 pages) memory is greater than the maximum allowed memory (65536 pages)", error));
  free(error);

  wasm_memorytype_delete(memtype7);

  printf("Shutting down...\n");
  wasm_store_delete(store);
  wasm_engine_delete(engine);

  printf("Done.\n");
  return 0;
}
