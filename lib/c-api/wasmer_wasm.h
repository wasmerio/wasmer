// This header file is for Wasmer APIs intended to be used with the standard Wasm C API.

#ifndef WASMER_WASM_H
#define WASMER_WASM_H

#include <stdint.h>
#include "wasm.h"

#define own

// In order to use WASI, we need a `wasi_env_t`, but first we need a `wasi_state_t`.
//
// We get a `wasi_state_t` by building it with the `wasi_state_builder_t`.
// Once we have a `wasi_state_t`, we can use `wasi_env_new` to create a `wasi_env_t`.
//
// Once we have a `wasi_env_t` we must:
// - set it up with `wasi_env_set_memory` to expose a memory to the WASI host functions
// - call `wasi_get_imports` to get an array of imports needed to instantiate the Wasm module.

// Used to build a `wasi_state_t`.
typedef struct wasi_state_builder_t wasi_state_builder_t;
// An opaque file handle to a WASI file.
typedef struct wasi_file_handle_t wasi_file_handle_t;
// The core WASI data structure, used to create a `wasi_env_t`.
typedef struct wasi_state_t wasi_state_t;
// This type is passed to the WASI host functions and owns a `wasi_state_t`. 
typedef struct wasi_env_t wasi_env_t;

// The version of WASI to use.
typedef uint32_t wasi_version_t;

enum {
  WASI_VERSION_LATEST = 0,
  WASI_VERSION_SNAPSHOT0 = 1,
  WASI_VERSION_SNAPSHOT1 = 2,
  WASI_VERSION_INVALID = ~0
};

// Create a `wasi_state_builder_t`.
//
// Takes as an argument the name of the Wasm program to execute (will show up
// as argv[0] to the Wasm program).
own wasi_state_builder_t* wasi_state_builder_new(const char* program_name);

// Add an argument to be passed to the Wasi program.
void wasi_state_builder_arg(wasi_state_builder_t*, const char* arg);

// Add an environment variable to be passed to the Wasi program.
void wasi_state_builder_env(wasi_state_builder_t*, const char* key, const char* value);

// Override `stdout` with the given `wasi_file_handle_t`.
void wasi_state_builder_set_stdout(wasi_state_builder_t*, wasi_file_handle_t*);

// Consume the `wasi_state_builder_t` and get a `wasi_state_t`.
own wasi_state_t* wasi_state_builder_build(own wasi_state_builder_t*);

// Create a `wasi_env_t`.
own wasi_env_t* wasi_env_new(own wasi_state_t*);

// Delete the `wasi_env_t`, used to clean up all the resources used by WASI.
void wasi_env_delete(own wasi_env_t*);

// Get an array of imports that can be used to instantiate the given module.
own const wasm_extern_t* own const* wasi_get_imports(wasm_store_t* store,
                                                     wasm_module_t* module,
                                                     wasi_env_t* wasi_env,
                                                     wasi_version_t version);

// TODO: investigate removing this part of the API
// TODO: investigate removing the wasi_version stuff from the API
// Set the memory in the `wasi_env_t` so that the WASI host functions can access WASI's memory.
void wasi_env_set_memory(wasi_env_t*, const wasm_memory_t*);

// Get temporary access to `wasi_state_t` owned by the given `wasi_env_t`.
wasi_state_t* wasi_env_borrow_state(const wasi_env_t*);

// Get the version of WASI needed by the given Wasm module.
wasi_version_t wasi_get_wasi_version(wasm_module_t*);

// TODO: consider using a circular buffer and making read a mutable operation to
// avoid wasted memory.
// Create a capturing WASI file. This file stores all data written to it.
own wasi_file_handle_t* wasi_output_capturing_file_new();

// Delete an owned `wasi_file_handle_t`
void wasi_output_capturing_file_delete(own wasi_file_handle_t*);

// Read from a capturing file (created by `wasi_output_capturing_file_new`).
size_t wasi_output_capturing_file_read(wasi_file_handle_t* file,
                                       char* buffer,
                                       size_t buffer_len,
                                       size_t start_offset);

// Get temporary access to the `stdout` WASI file.
wasi_file_handle_t* wasi_state_get_stdout(wasi_state_t*);

// TODO: figure out if we can do less duplication.
/**
 * Gets the length in bytes of the last error if any.
 *
 * This can be used to dynamically allocate a buffer with the correct number of
 * bytes needed to store a message.
 *
 * See `wasmer_last_error_message()` to get a full example.
 */
int wasmer_last_error_length();

/**
 * Gets the last error message if any into the provided buffer
 * `buffer` up to the given `length`.
 *
 * The `length` parameter must be large enough to store the last
 * error message. Ideally, the value should come from
 * `wasmer_last_error_length()`.
 *
 * The function returns the length of the string in bytes, `-1` if an
 * error occurs. Potential errors are:
 *
 *  * The buffer is a null pointer,
 *  * The buffer is too small to hold the error message.
 *
 * Note: The error message always has a trailing null character.
 *
 * Example:
 *
 * ```c
 * int error_length = wasmer_last_error_length();
 *
 * if (error_length > 0) {
 *     char *error_message = malloc(error_length);
 *     wasmer_last_error_message(error_message, error_length);
 *     printf("Error message: `%s`\n", error_message);
 * } else {
 *     printf("No error message\n");
 * }
 * ```
 */
int wasmer_last_error_message(char* buffer, int length);

#endif /* WASMER_WASM_H */
