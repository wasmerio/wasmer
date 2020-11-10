#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

#include "wasmer_wasm.h"

#define own

int main(int argc, const char* argv[]) {
  // Initialize.
  printf("Initializing...\n");
  wasm_engine_t* engine = wasm_engine_new();
  wasm_store_t* store = wasm_store_new(engine);

  // Getting Wasm.
  printf("Compiling WAT to Wasm...\n");
  
  wasm_byte_vec_t wat = {
    .data = "(module)",
    .size = 8,
  };
  wasm_byte_vec_t *wasm = wat2wasm(&wat);

  if (!wasm) {
    printf("> Error compiler WAT to Wasm!\n");
    return 1;
  }

  if (wasm->size != 8) {
    printf("The Wasm size is incorrect!\n");
    return 1;
  }

  if (!(wasm->data[0] == 0 &&
        wasm->data[1] == 'a' &&
        wasm->data[2] == 's' &&
        wasm->data[3] == 'm' &&
        wasm->data[4] == 1 &&
        wasm->data[5] == 0 &&
        wasm->data[6] == 0 &&
        wasm->data[7] == 0)) {
    printf("The Wasm data is incorrect!\n");
    return 1;
  }

  wasm_byte_vec_delete(wasm);
  wasm_byte_vec_delete(&wat);

  // All done.
  printf("Done.\n");
  return 0;
}
