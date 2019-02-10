#ifndef WASMER_H
#define WASMER_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef enum {
  WASMER_CALL_OK = 1,
  WASMER_CALL_ERROR = 2,
} wasmer_call_result_t;

typedef enum {
  WASMER_COMPILE_OK = 1,
  WASMER_COMPILE_ERROR = 2,
} wasmer_compile_result_t;

typedef enum {
  WASMER_MEMORY_OK = 1,
  WASMER_MEMORY_ERROR = 2,
} wasmer_memory_result_t;

typedef enum {
  WASMER_TABLE_OK = 1,
  WASMER_TABLE_ERROR = 2,
} wasmer_table_result_t;

enum wasmer_value_tag {
  WASM_I32,
  WASM_I64,
  WASM_F32,
  WASM_F64,
};
typedef uint32_t wasmer_value_tag;

typedef struct wasmer_import_object_t wasmer_import_object_t;

typedef struct wasmer_instance_context_t wasmer_instance_context_t;

typedef struct wasmer_instance_t wasmer_instance_t;

typedef struct {

} wasmer_global_t;

typedef union {
  int32_t I32;
  int64_t I64;
  float F32;
  double F64;
} wasmer_value;

typedef struct {
  wasmer_value_tag tag;
  wasmer_value value;
} wasmer_value_t;

typedef struct {
  bool mutable_;
  wasmer_value_tag kind;
} wasmer_global_descriptor_t;

typedef struct {

} wasmer_memory_t;

typedef struct {
  uint32_t min;
  uint32_t max;
} wasmer_limits_t;

typedef struct {

} wasmer_table_t;

void wasmer_global_destroy(wasmer_global_t *global);

wasmer_value_t wasmer_global_get(wasmer_global_t *global);

wasmer_global_descriptor_t wasmer_global_get_descriptor(wasmer_global_t *global);

wasmer_global_t *wasmer_global_new(wasmer_value_t value, bool mutable_);

void wasmer_global_set(wasmer_global_t *global, wasmer_value_t value);

void wasmer_import_object_destroy(wasmer_import_object_t *import_object);

wasmer_import_object_t *wasmer_import_object_new(void);

void wasmer_imports_set_import_func(wasmer_import_object_t *import_object,
                                    const char *namespace_,
                                    const char *name,
                                    void (*func)(void *data),
                                    const wasmer_value_tag *params,
                                    int params_len,
                                    const wasmer_value_tag *returns,
                                    int returns_len);

wasmer_call_result_t wasmer_instance_call(wasmer_instance_t *instance,
                                          const char *name,
                                          const wasmer_value_t *params,
                                          int params_len,
                                          wasmer_value_t *results,
                                          int results_len);

const wasmer_memory_t *wasmer_instance_context_memory(wasmer_instance_context_t *ctx,
                                                      uint32_t memory_idx);

void wasmer_instance_destroy(wasmer_instance_t *instance);

wasmer_compile_result_t wasmer_instantiate(wasmer_instance_t **instance,
                                           uint8_t *wasm_bytes,
                                           uint32_t wasm_bytes_len,
                                           wasmer_import_object_t *import_object);

int wasmer_last_error_length(void);

int wasmer_last_error_message(char *buffer, int length);

uint8_t *wasmer_memory_data(wasmer_memory_t *mem);

uint32_t wasmer_memory_data_length(wasmer_memory_t *mem);

void wasmer_memory_destroy(wasmer_memory_t *memory);

wasmer_memory_result_t wasmer_memory_grow(wasmer_memory_t *memory, uint32_t delta);

uint32_t wasmer_memory_length(wasmer_memory_t *memory);

wasmer_memory_result_t wasmer_memory_new(wasmer_memory_t **memory, wasmer_limits_t limits);

void wasmer_table_destroy(wasmer_table_t *table);

wasmer_table_result_t wasmer_table_grow(wasmer_table_t *table, uint32_t delta);

uint32_t wasmer_table_length(wasmer_table_t *table);

wasmer_table_result_t wasmer_table_new(wasmer_table_t **table, wasmer_limits_t limits);

bool wasmer_validate(uint8_t *wasm_bytes, uint32_t wasm_bytes_len);

#endif /* WASMER_H */
