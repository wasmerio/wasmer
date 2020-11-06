// This header file is used only for test purposes! It is used by unit
// test inside the `src/` directory for the moment.

#include <assert.h>
#include <string.h>
#include "../wasmer_wasm.h"

// Wasmer-specific shortcut to quickly create a `wasm_byte_vec_t` from
// a string.
static inline void wasm_byte_vec_new_from_string(
  wasm_byte_vec_t* out, const char* s
) {
  wasm_byte_vec_new(out, strlen(s), s);
}
