// This header file is for Wasmer APIs intended to be used with the standard Wasm C API.

#ifndef WASMER_WASM_H
#define WASMER_WASM_H

#include <stdint.h>
#include "wasm.h"

#define own

// In order to use WASI, we need a `wasi_env_t`, but first we need to configure it with
// a `wasi_config_t`.
//
// We get a `wasi_config_t` by building it with the `wasi_config_new` function and
// from there we can set arguments, environment variables, and standard file behavior.
// Then we can call `wasi_env_new` with the `wasi_config_t` and get a `wasi_env_t`.
//
// Once we have a `wasi_env_t` we must:
// - set it up with `wasi_env_set_memory` to expose a memory to the WASI host functions
// - call `wasi_get_imports` to get an array of imports needed to instantiate the Wasm module.

// Used to build a `wasi_env_t`.
typedef struct wasi_config_t wasi_config_t;
// This type is passed to the WASI host functions owns the data core to the
// functioning of WASI.
typedef struct wasi_env_t wasi_env_t;

// The version of WASI to use.
typedef uint32_t wasi_version_t;

enum {
  WASI_VERSION_LATEST = 0,
  WASI_VERSION_SNAPSHOT0 = 1,
  WASI_VERSION_SNAPSHOT1 = 2,
  WASI_VERSION_INVALID = ~0
};

// Create a `wasi_config_t`.
//
// Takes as an argument the name of the Wasm program to execute (will show up
// as argv[0] to the Wasm program).
own wasi_config_t* wasi_config_new(const char* program_name);

// Add an argument to be passed to the Wasi program.
void wasi_config_arg(wasi_config_t*, const char* arg);

// Add an environment variable to be passed to the Wasi program.
void wasi_config_env(wasi_config_t*, const char* key, const char* value);

// Have the WASI program print directly to stdout
void wasi_config_inherit_stdout(wasi_config_t*);

// Have the WASI program print directly to stderr
void wasi_config_inherit_stderr(wasi_config_t*);

// Have the WASI program read directly to stdin
//void wasi_config_inherit_stdin(wasi_config_t*);

// Create a `wasi_env_t`.
own wasi_env_t* wasi_env_new(own wasi_config_t*);

// Delete the `wasi_env_t`, used to clean up all the resources used by WASI.
void wasi_env_delete(own wasi_env_t*);

// Get an array of imports that can be used to instantiate the given module.
bool wasi_get_imports(wasm_store_t* store,
                      const wasm_module_t* module,
                      wasi_env_t* wasi_env,
                      wasm_extern_t** imports);

// Set up the `wasi_env_t` so that the WASI host functions can access WASI's memory.
// Returns whether or not it succeeded.
bool wasi_env_set_instance(wasi_env_t*, const wasm_instance_t*);

// Set the memory in the `wasi_env_t` so that the WASI host functions can access WASI's memory.
// Returns whether or not it succeeded.
void wasi_env_set_memory(wasi_env_t*, const wasm_memory_t*);

// Read from WASI's buffered stdout if stdout has not been inherited with
// `wasi_config_inherit_stdout`.
size_t wasi_env_read_stdout(wasi_env_t* env,
                            char* buffer,
                            size_t buffer_len);

// Read from WASI's buffered stderr if stdout has not been inherited with
// `wasi_config_inherit_stderr`.
size_t wasi_env_read_stderr(wasi_env_t* env,
                            char* buffer,
                            size_t buffer_len);

// Get the version of WASI needed by the given Wasm module.
wasi_version_t wasi_get_wasi_version(wasm_module_t*);

// Get the start function which initializes the WASI state and calls main.
//
// The start function takes 0 arguments and returns 0 values.
own wasm_func_t* wasi_get_start_function(wasm_instance_t*);

// Delete a `wasm_extern_t` allocated by the API.
void wasm_extern_delete(own wasm_extern_t*);

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
 * Note: The error message always has a trailing NUL character.
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
