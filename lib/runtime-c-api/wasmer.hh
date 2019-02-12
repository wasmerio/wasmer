#ifndef WASMER_H
#define WASMER_H

#include <cstdarg>
#include <cstdint>
#include <cstdlib>

enum class wasmer_result_t {
  WASMER_OK = 1,
  WASMER_ERROR = 2,
};

enum class wasmer_value_tag : uint32_t {
  WASM_I32,
  WASM_I64,
  WASM_F32,
  WASM_F64,
};

struct wasmer_import_object_t;

struct wasmer_instance_context_t;

struct wasmer_instance_t;

struct wasmer_global_t {

};

union wasmer_value {
  int32_t I32;
  int64_t I64;
  float F32;
  double F64;
};

struct wasmer_value_t {
  wasmer_value_tag tag;
  wasmer_value value;
};

struct wasmer_global_descriptor_t {
  bool mutable_;
  wasmer_value_tag kind;
};

struct wasmer_memory_t {

};

struct wasmer_limits_t {
  uint32_t min;
  uint32_t max;
};

struct wasmer_table_t {

};

extern "C" {

void wasmer_global_destroy(wasmer_global_t *global);

wasmer_value_t wasmer_global_get(wasmer_global_t *global);

wasmer_global_descriptor_t wasmer_global_get_descriptor(wasmer_global_t *global);

wasmer_global_t *wasmer_global_new(wasmer_value_t value, bool mutable_);

void wasmer_global_set(wasmer_global_t *global, wasmer_value_t value);

void wasmer_import_object_destroy(wasmer_import_object_t *import_object);

wasmer_import_object_t *wasmer_import_object_new();

void wasmer_imports_set_import_func(wasmer_import_object_t *import_object,
                                    const char *namespace_,
                                    const char *name,
                                    void (*func)(void *data),
                                    const wasmer_value_tag *params,
                                    int params_len,
                                    const wasmer_value_tag *returns,
                                    int returns_len);

wasmer_result_t wasmer_instance_call(wasmer_instance_t *instance,
                                     const char *name,
                                     const wasmer_value_t *params,
                                     int params_len,
                                     wasmer_value_t *results,
                                     int results_len);

const wasmer_memory_t *wasmer_instance_context_memory(wasmer_instance_context_t *ctx,
                                                      uint32_t memory_idx);

void wasmer_instance_destroy(wasmer_instance_t *instance);

wasmer_result_t wasmer_instantiate(wasmer_instance_t **instance,
                                   uint8_t *wasm_bytes,
                                   uint32_t wasm_bytes_len,
                                   wasmer_import_object_t *import_object);

int wasmer_last_error_length();

int wasmer_last_error_message(char *buffer, int length);

uint8_t *wasmer_memory_data(wasmer_memory_t *mem);

uint32_t wasmer_memory_data_length(wasmer_memory_t *mem);

void wasmer_memory_destroy(wasmer_memory_t *memory);

wasmer_result_t wasmer_memory_grow(wasmer_memory_t *memory, uint32_t delta);

uint32_t wasmer_memory_length(wasmer_memory_t *memory);

wasmer_result_t wasmer_memory_new(wasmer_memory_t **memory, wasmer_limits_t limits);

void wasmer_table_destroy(wasmer_table_t *table);

wasmer_result_t wasmer_table_grow(wasmer_table_t *table, uint32_t delta);

uint32_t wasmer_table_length(wasmer_table_t *table);

wasmer_result_t wasmer_table_new(wasmer_table_t **table, wasmer_limits_t limits);

bool wasmer_validate(uint8_t *wasm_bytes, uint32_t wasm_bytes_len);

} // extern "C"

#endif // WASMER_H
