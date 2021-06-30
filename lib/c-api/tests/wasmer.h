// This header file is used only for test purposes! It is used by unit
// test inside the `src/` directory for the moment.

#ifndef TEST_WASMER
#define TEST_WASMER

#include "../wasmer.h"
#include "wasm.h"
#include <stdio.h>
#include <string.h>

// Assert that a `wasm_name_t` equals something.
void wasmer_assert_name(const wasm_name_t *name, const char *expected) {
  assert(name->size == strlen(expected) &&
         strncmp(name->data, expected, name->size) == 0);
}

// Helper to quickly create a `wasm_byte_vec_t` from a string, à la
// `wasm_name_new_from_string`.
static inline void wasmer_byte_vec_new_from_string(wasm_byte_vec_t *out,
                                                   const char *s) {
  wasm_byte_vec_new(out, strlen(s), s);
}

#endif /* TEST_WASMER */
