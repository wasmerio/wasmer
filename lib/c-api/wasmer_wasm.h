// The Wasmer C/C++ header file compatible with the `wasm-c-api` standard API.

#if !defined(WASMER_WASM_H_MACROS)

#define WASMER_WASM_H_MACROS

// Define the `ARCH_X86_X64` constant.
#if defined(MSVC) && defined(_M_AMD64)
#  define ARCH_X86_64
#elif (defined(GCC) || defined(__GNUC__) || defined(__clang__)) && defined(__x86_64__)
#  define ARCH_X86_64
#endif

// Compatibility with non-Clang compilers.
#if !defined(__has_attribute)
#  define __has_attribute(x) 0
#endif

// Compatibility with non-Clang compilers.
#if !defined(__has_declspec_attribute)
#  define __has_declspec_attribute(x) 0
#endif

// Define the `DEPRECATED` macro.
#if defined(GCC) || defined(__GNUC__) || __has_attribute(deprecated)
#  define DEPRECATED(message) __attribute__((deprecated(message)))
#elif defined(MSVC) || __has_declspec_attribute(deprecated)
#  define DEPRECATED(message) __declspec(deprecated(message))
#endif

// The `jit` feature has been enabled for this build.
#define WASMER_JIT_ENABLED

// The `compiler` feature has been enabled for this build.
#define WASMER_COMPILER_ENABLED

// The `wasi` feature has been enabled for this build.
#define WASMER_WASI_ENABLED

#endif // WASMER_WASM_H_MACROS


//
// OK, here we go. The code below is automatically generated.
//


#ifndef WASMER_WASM_H
#define WASMER_WASM_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include "wasm.h"

/**
 * this can be a wasmer-specific type with wasmer-specific functions for manipulating it
 */
typedef enum {
  CRANELIFT = 0,
  LLVM = 1,
  SINGLEPASS = 2,
} wasmer_compiler_t;

typedef enum {
  JIT = 0,
  NATIVE = 1,
  OBJECT_FILE = 2,
} wasmer_engine_t;

#if defined(WASMER_WASI_ENABLED)
typedef struct wasi_config_t wasi_config_t;
#endif

#if defined(WASMER_WASI_ENABLED)
typedef struct wasi_env_t wasi_env_t;
#endif

#if defined(WASMER_WASI_ENABLED)
typedef struct wasi_version_t wasi_version_t;
#endif

#if defined(WASMER_WASI_ENABLED)
void wasi_config_arg(wasi_config_t *config, const char *arg);
#endif

#if defined(WASMER_WASI_ENABLED)
void wasi_config_env(wasi_config_t *config, const char *key, const char *value);
#endif

#if defined(WASMER_WASI_ENABLED)
void wasi_config_inherit_stderr(wasi_config_t *config);
#endif

#if defined(WASMER_WASI_ENABLED)
void wasi_config_inherit_stdin(wasi_config_t *config);
#endif

#if defined(WASMER_WASI_ENABLED)
void wasi_config_inherit_stdout(wasi_config_t *config);
#endif

#if defined(WASMER_WASI_ENABLED)
bool wasi_config_mapdir(wasi_config_t *config, const char *alias, const char *dir);
#endif

#if defined(WASMER_WASI_ENABLED)
wasi_config_t *wasi_config_new(const char *program_name);
#endif

#if defined(WASMER_WASI_ENABLED)
bool wasi_config_preopen_dir(wasi_config_t *config, const char *dir);
#endif

#if defined(WASMER_WASI_ENABLED)
void wasi_env_delete(wasi_env_t *_state);
#endif

#if defined(WASMER_WASI_ENABLED)
/**
 * Takes ownership over the `wasi_config_t`.
 */
wasi_env_t *wasi_env_new(wasi_config_t *config);
#endif

#if defined(WASMER_WASI_ENABLED)
intptr_t wasi_env_read_stderr(wasi_env_t *env, char *buffer, uintptr_t buffer_len);
#endif

#if defined(WASMER_WASI_ENABLED)
intptr_t wasi_env_read_stdout(wasi_env_t *env, char *buffer, uintptr_t buffer_len);
#endif

#if defined(WASMER_WASI_ENABLED)
bool wasi_env_set_instance(wasi_env_t *env, const wasm_instance_t *instance);
#endif

#if defined(WASMER_WASI_ENABLED)
void wasi_env_set_memory(wasi_env_t *env, const wasm_memory_t *memory);
#endif

#if defined(WASMER_WASI_ENABLED)
/**
 * Takes ownership of `wasi_env_t`.
 */
bool wasi_get_imports(const wasm_store_t *store,
                      const wasm_module_t *module,
                      const wasi_env_t *wasi_env,
                      wasm_extern_vec_t *imports);
#endif

#if defined(WASMER_WASI_ENABLED)
wasm_func_t *wasi_get_start_function(wasm_instance_t *instance);
#endif

#if defined(WASMER_WASI_ENABLED)
wasi_version_t wasi_get_wasi_version(const wasm_module_t *module);
#endif

void wasm_config_set_compiler(wasm_config_t *config, wasmer_compiler_t compiler);

void wasm_config_set_engine(wasm_config_t *config, wasmer_engine_t engine);

void wasm_module_name(const wasm_module_t *module, wasm_name_t *out);

bool wasm_module_set_name(wasm_module_t *module, const wasm_name_t *name);

/**
 * Gets the length in bytes of the last error if any.
 *
 * This can be used to dynamically allocate a buffer with the correct number of
 * bytes needed to store a message.
 *
 * See `wasmer_last_error_message()` to get a full example.
 */
int wasmer_last_error_length(void);

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
int wasmer_last_error_message(char *buffer, int length);

/**
 * Parses in-memory bytes as either the WAT format, or a binary Wasm
 * module. This is wasmer-specific.
 *
 * In case of failure, `wat2wasm` returns `NULL`.
 */
wasm_byte_vec_t *wat2wasm(const wasm_byte_vec_t *wat);

#endif /* WASMER_WASM_H */
