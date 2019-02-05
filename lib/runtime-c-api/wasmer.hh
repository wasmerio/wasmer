#include <cstdarg>
#include <cstdint>
#include <cstdlib>

enum class wasmer_call_result_t {
  WASMER_CALL_OK = 1,
  WASMER_CALL_ERROR = 2,
};

enum class wasmer_compile_result_t {
  WASMER_COMPILE_OK = 1,
  WASMER_COMPILE_ERROR = 2,
};

enum class wasmer_memory_result_t {
  WASMER_MEMORY_OK = 1,
  WASMER_MEMORY_ERROR = 2,
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

struct wasmer_memory_t {

};

struct wasmer_limits_t {
  uint32_t min;
  uint32_t max;
};

extern "C" {

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

wasmer_call_result_t wasmer_instance_call(wasmer_instance_t *instance,
                                          const char *name,
                                          const wasmer_value_t *params,
                                          int params_len,
                                          wasmer_value_t *results,
                                          int results_len);

void wasmer_instance_context_memory(wasmer_instance_context_t *instance);

void wasmer_instance_destroy(wasmer_instance_t *instance);

wasmer_compile_result_t wasmer_instantiate(wasmer_instance_t **instance,
                                           uint8_t *wasm_bytes,
                                           uint32_t wasm_bytes_len,
                                           wasmer_import_object_t *import_object);

void wasmer_memory_destroy(wasmer_memory_t *memory);

uint32_t wasmer_memory_length(wasmer_memory_t *memory);

wasmer_memory_result_t wasmer_memory_new(wasmer_memory_t **memory, wasmer_limits_t limits);

} // extern "C"
