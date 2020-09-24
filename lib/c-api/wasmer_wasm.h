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

enum wasm_valkind_enum {
  WASM_I32 = 0,
  WASM_I64 = 1,
  WASM_F32 = 2,
  WASM_F64 = 3,
  WASM_ANYREF = 128,
  WASM_FUNCREF = 129,
};
typedef uint8_t wasm_valkind_enum;

typedef struct Box_wasi_config_t Box_wasi_config_t;

typedef struct Box_wasm_engine_t Box_wasm_engine_t;

typedef struct Box_wasm_exporttype_t Box_wasm_exporttype_t;

typedef struct Box_wasm_externtype_t Box_wasm_externtype_t;

typedef struct Box_wasm_global_t Box_wasm_global_t;

typedef struct Box_wasm_importtype_t Box_wasm_importtype_t;

typedef struct Box_wasm_memory_t Box_wasm_memory_t;

typedef struct Box_wasm_memorytype_t Box_wasm_memorytype_t;

typedef struct Box_wasm_table_t Box_wasm_table_t;

typedef struct Box_wasm_tabletype_t Box_wasm_tabletype_t;

typedef struct Box_wasm_valtype_t Box_wasm_valtype_t;

typedef struct Option_Box_wasi_config_t Option_Box_wasi_config_t;

typedef struct Option_Box_wasi_env_t Option_Box_wasi_env_t;

typedef struct Option_Box_wasm_engine_t Option_Box_wasm_engine_t;

typedef struct Option_Box_wasm_extern_t Option_Box_wasm_extern_t;

typedef struct Option_Box_wasm_externtype_t Option_Box_wasm_externtype_t;

typedef struct Option_Box_wasm_func_t Option_Box_wasm_func_t;

typedef struct Option_Box_wasm_functype_t Option_Box_wasm_functype_t;

typedef struct Option_Box_wasm_global_t Option_Box_wasm_global_t;

typedef struct Option_Box_wasm_globaltype_t Option_Box_wasm_globaltype_t;

typedef struct Option_Box_wasm_importtype_t Option_Box_wasm_importtype_t;

typedef struct Option_Box_wasm_instance_t Option_Box_wasm_instance_t;

typedef struct Option_Box_wasm_memory_t Option_Box_wasm_memory_t;

typedef struct Option_Box_wasm_memorytype_t Option_Box_wasm_memorytype_t;

typedef struct Option_Box_wasm_module_t Option_Box_wasm_module_t;

typedef struct Option_Box_wasm_table_t Option_Box_wasm_table_t;

typedef struct Option_Box_wasm_tabletype_t Option_Box_wasm_tabletype_t;

typedef struct Option_Box_wasm_valtype_t Option_Box_wasm_valtype_t;

#if defined(WASMER_WASI_ENABLED)
typedef struct wasi_version_t wasi_version_t;
#endif

typedef struct wasm_ref_t wasm_ref_t;

#if defined(WASMER_WASI_ENABLED)
typedef struct {
  bool inherit_stdout;
  bool inherit_stderr;
  bool inherit_stdin;
} wasi_config_t;
#endif

#if defined(WASMER_WASI_ENABLED)
typedef struct {

} wasi_env_t;
#endif

typedef struct {

} wasm_instance_t;

typedef struct {

} wasm_memory_t;

/**
 * Opaque wrapper around `Store`
 */
typedef struct {

} wasm_store_t;

typedef struct {

} wasm_module_t;

typedef struct {

} wasm_extern_t;

/**
 * this can be a wasmer-specific type with wasmer-specific functions for manipulating it
 */
typedef struct {

} wasm_config_t;

typedef wasm_byte_vec_t wasm_name_t;

typedef struct {

} wasm_externtype_t;

typedef struct {
  wasm_name_t *name;
  wasm_externtype_t *extern_type;
} wasm_exporttype_t;

typedef uint8_t wasm_externkind_t;

typedef struct {

} wasm_functype_t;

typedef struct {

} wasm_globaltype_t;

typedef struct {

} wasm_memorytype_t;

typedef struct {

} wasm_tabletype_t;

typedef struct {

} wasm_func_t;

typedef struct {

} wasm_trap_t;

typedef uint8_t wasm_valkind_t;

typedef union {
  int32_t int32_t;
  int64_t int64_t;
  float float32_t;
  double float64_t;
  wasm_ref_t *wref;
} wasm_val_inner;

typedef struct {
  wasm_valkind_t kind;
  wasm_val_inner of;
} wasm_val_t;

typedef wasm_trap_t *(*wasm_func_callback_t)(const wasm_val_t *args, wasm_val_t *results);

typedef wasm_trap_t *(*wasm_func_callback_with_env_t)(void*, const wasm_val_t *args, wasm_val_t *results);

typedef void (*wasm_env_finalizer_t)(void);

typedef struct {

} wasm_global_t;

typedef struct {
  wasm_valkind_enum valkind;
} wasm_valtype_t;

typedef uint8_t wasm_mutability_t;

typedef struct {
  wasm_name_t *module;
  wasm_name_t *name;
  wasm_externtype_t *extern_type;
} wasm_importtype_t;

typedef struct {
  uint32_t min;
  uint32_t max;
} wasm_limits_t;

typedef struct {

} wasm_engine_t;

typedef struct {

} wasm_table_t;

typedef uint32_t wasm_table_size_t;

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
Option_Box_wasi_config_t wasi_config_new(const char *program_name);
#endif

#if defined(WASMER_WASI_ENABLED)
void wasi_env_delete(Option_Box_wasi_env_t _state);
#endif

#if defined(WASMER_WASI_ENABLED)
/**
 * Takes ownership over the `wasi_config_t`.
 */
Option_Box_wasi_env_t wasi_env_new(Box_wasi_config_t config);
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
bool wasi_get_imports(wasm_store_t *store,
                      const wasm_module_t *module,
                      const wasi_env_t *wasi_env,
                      wasm_extern_t **imports);
#endif

#if defined(WASMER_WASI_ENABLED)
wasi_version_t wasi_get_wasi_version(const wasm_module_t *module);
#endif

wasm_config_t *wasm_config_new(void);

void wasm_engine_delete(Option_Box_wasm_engine_t _wasm_engine_address);

Box_wasm_engine_t wasm_engine_new_with_config(wasm_config_t *_config_ptr);

const wasm_name_t *wasm_exporttype_name(const wasm_exporttype_t *et);

Box_wasm_exporttype_t wasm_exporttype_new(wasm_name_t *name, wasm_externtype_t *extern_type);

const wasm_externtype_t *wasm_exporttype_type(const wasm_exporttype_t *et);

Option_Box_wasm_func_t wasm_extern_as_func(wasm_extern_t *extern_ptr);

Option_Box_wasm_global_t wasm_extern_as_global(wasm_extern_t *extern_ptr);

Option_Box_wasm_memory_t wasm_extern_as_memory(wasm_extern_t *extern_ptr);

Option_Box_wasm_table_t wasm_extern_as_table(wasm_extern_t *extern_ptr);

wasm_externkind_t wasm_extern_kind(const wasm_extern_t *e);

Box_wasm_externtype_t wasm_extern_type(const wasm_extern_t *e);

const wasm_functype_t *wasm_externtype_as_functype(const wasm_externtype_t *et);

const wasm_functype_t *wasm_externtype_as_functype_const(const wasm_externtype_t *et);

const wasm_globaltype_t *wasm_externtype_as_globaltype(const wasm_externtype_t *et);

const wasm_globaltype_t *wasm_externtype_as_globaltype_const(const wasm_externtype_t *et);

const wasm_memorytype_t *wasm_externtype_as_memorytype(const wasm_externtype_t *et);

const wasm_memorytype_t *wasm_externtype_as_memorytype_const(const wasm_externtype_t *et);

const wasm_tabletype_t *wasm_externtype_as_tabletype(const wasm_externtype_t *et);

const wasm_tabletype_t *wasm_externtype_as_tabletype_const(const wasm_externtype_t *et);

void wasm_externtype_delete(Option_Box_wasm_externtype_t _et);

wasm_externkind_t wasm_externtype_kind(const wasm_externtype_t *et);

Option_Box_wasm_extern_t wasm_func_as_extern(wasm_func_t *func_ptr);

wasm_trap_t *wasm_func_call(const wasm_func_t *func, const wasm_val_t *args, wasm_val_t *results);

void wasm_func_delete(Option_Box_wasm_func_t _func);

Option_Box_wasm_func_t wasm_func_new(wasm_store_t *store,
                                     const wasm_functype_t *ft,
                                     wasm_func_callback_t callback);

Option_Box_wasm_func_t wasm_func_new_with_env(wasm_store_t *store,
                                              const wasm_functype_t *ft,
                                              wasm_func_callback_with_env_t callback,
                                              void *env,
                                              wasm_env_finalizer_t finalizer);

uintptr_t wasm_func_param_arity(const wasm_func_t *func);

uintptr_t wasm_func_result_arity(const wasm_func_t *func);

const wasm_externtype_t *wasm_functype_as_externtype(const wasm_functype_t *ft);

const wasm_externtype_t *wasm_functype_as_externtype_const(const wasm_functype_t *ft);

Option_Box_wasm_functype_t wasm_functype_copy(wasm_functype_t *arg);

void wasm_functype_delete(Option_Box_wasm_functype_t _ft);

Option_Box_wasm_functype_t wasm_functype_new(wasm_valtype_vec_t *params,
                                             wasm_valtype_vec_t *results);

const wasm_valtype_vec_t *wasm_functype_params(const wasm_functype_t *ft);

const wasm_valtype_vec_t *wasm_functype_results(const wasm_functype_t *ft);

Option_Box_wasm_extern_t wasm_global_as_extern(wasm_global_t *global_ptr);

Box_wasm_global_t wasm_global_copy(const wasm_global_t *wasm_global);

void wasm_global_delete(Option_Box_wasm_global_t _global);

void wasm_global_get(const wasm_global_t *wasm_global, wasm_val_t *out);

Option_Box_wasm_global_t wasm_global_new(wasm_store_t *store_ptr,
                                         const wasm_globaltype_t *gt,
                                         const wasm_val_t *val);

bool wasm_global_same(const wasm_global_t *wasm_global1, const wasm_global_t *wasm_global2);

void wasm_global_set(wasm_global_t *wasm_global, const wasm_val_t *val);

const wasm_externtype_t *wasm_globaltype_as_externtype(const wasm_globaltype_t *gt);

const wasm_externtype_t *wasm_globaltype_as_externtype_const(const wasm_globaltype_t *gt);

const wasm_valtype_t *wasm_globaltype_content(const wasm_globaltype_t *globaltype);

void wasm_globaltype_delete(Option_Box_wasm_globaltype_t _globaltype);

wasm_mutability_t wasm_globaltype_mutability(const wasm_globaltype_t *globaltype);

Option_Box_wasm_globaltype_t wasm_globaltype_new(Option_Box_wasm_valtype_t valtype,
                                                 wasm_mutability_t mutability);

void wasm_importtype_delete(Option_Box_wasm_importtype_t _importtype);

const wasm_name_t *wasm_importtype_module(const wasm_importtype_t *et);

const wasm_name_t *wasm_importtype_name(const wasm_importtype_t *et);

Box_wasm_importtype_t wasm_importtype_new(wasm_name_t *module,
                                          wasm_name_t *name,
                                          wasm_externtype_t *extern_type);

const wasm_externtype_t *wasm_importtype_type(const wasm_importtype_t *et);

void wasm_instance_delete(Option_Box_wasm_instance_t _instance);

void wasm_instance_exports(const wasm_instance_t *instance, wasm_extern_vec_t *out);

Option_Box_wasm_instance_t wasm_instance_new(wasm_store_t *store,
                                             const wasm_module_t *module,
                                             const wasm_extern_t *const *imports,
                                             wasm_trap_t **_traps);

Option_Box_wasm_extern_t wasm_memory_as_extern(wasm_memory_t *memory_ptr);

Box_wasm_memory_t wasm_memory_copy(const wasm_memory_t *wasm_memory);

uint8_t *wasm_memory_data(wasm_memory_t *memory);

uintptr_t wasm_memory_data_size(const wasm_memory_t *memory);

void wasm_memory_delete(Option_Box_wasm_memory_t _memory);

bool wasm_memory_grow(wasm_memory_t *memory, uint32_t delta);

Option_Box_wasm_memory_t wasm_memory_new(wasm_store_t *store_ptr, const wasm_memorytype_t *mt);

bool wasm_memory_same(const wasm_memory_t *wasm_memory1, const wasm_memory_t *wasm_memory2);

uint32_t wasm_memory_size(const wasm_memory_t *memory);

wasm_memorytype_t *wasm_memory_type(const wasm_memory_t *_memory_ptr);

const wasm_externtype_t *wasm_memorytype_as_externtype(const wasm_memorytype_t *mt);

const wasm_externtype_t *wasm_memorytype_as_externtype_const(const wasm_memorytype_t *mt);

void wasm_memorytype_delete(Option_Box_wasm_memorytype_t _memorytype);

const wasm_limits_t *wasm_memorytype_limits(const wasm_memorytype_t *mt);

Box_wasm_memorytype_t wasm_memorytype_new(const wasm_limits_t *limits);

void wasm_module_delete(Option_Box_wasm_module_t _module);

wasm_module_t *wasm_module_deserialize(wasm_store_t *store_ptr, const wasm_byte_vec_t *bytes);

void wasm_module_exports(const wasm_module_t *module, wasm_exporttype_vec_t *out);

void wasm_module_imports(const wasm_module_t *module, wasm_importtype_vec_t *out);

Option_Box_wasm_module_t wasm_module_new(wasm_store_t *store_ptr, const wasm_byte_vec_t *bytes);

void wasm_module_serialize(const wasm_module_t *module, wasm_byte_vec_t *out_ptr);

void wasm_store_delete(wasm_store_t *wasm_store);

wasm_store_t *wasm_store_new(wasm_engine_t *wasm_engine_ptr);

Option_Box_wasm_extern_t wasm_table_as_extern(wasm_table_t *table_ptr);

Box_wasm_table_t wasm_table_copy(const wasm_table_t *wasm_table);

void wasm_table_delete(Option_Box_wasm_table_t _table);

bool wasm_table_grow(wasm_table_t *_wasm_table, wasm_table_size_t _delta, wasm_ref_t *_init);

Option_Box_wasm_table_t wasm_table_new(wasm_store_t *store_ptr,
                                       const wasm_tabletype_t *tt,
                                       const wasm_ref_t *init);

bool wasm_table_same(const wasm_table_t *wasm_table1, const wasm_table_t *wasm_table2);

uintptr_t wasm_table_size(const wasm_table_t *wasm_table);

const wasm_externtype_t *wasm_tabletype_as_externtype(const wasm_tabletype_t *tt);

const wasm_externtype_t *wasm_tabletype_as_externtype_const(const wasm_tabletype_t *tt);

void wasm_tabletype_delete(Option_Box_wasm_tabletype_t _tabletype);

const wasm_valtype_t *wasm_tabletype_element(const wasm_tabletype_t *tabletype);

const wasm_limits_t *wasm_tabletype_limits(const wasm_tabletype_t *tabletype);

Box_wasm_tabletype_t wasm_tabletype_new(Box_wasm_valtype_t valtype, const wasm_limits_t *limits);

void wasm_trap_delete(wasm_trap_t *trap);

void wasm_trap_message(const wasm_trap_t *trap, wasm_byte_vec_t *out_ptr);

void wasm_val_copy(wasm_val_t *out_ptr, const wasm_val_t *val);

void wasm_val_delete(wasm_val_t *val);

void wasm_valtype_delete(Option_Box_wasm_valtype_t _valtype);

wasm_valkind_t wasm_valtype_kind(const wasm_valtype_t *valtype);

Option_Box_wasm_valtype_t wasm_valtype_new(wasm_valkind_t kind);

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

#endif /* WASMER_WASM_H */
