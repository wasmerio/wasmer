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

typedef struct wasmer_import_object_t wasmer_import_object_t;

typedef struct wasmer_instance_t wasmer_instance_t;

void wasmer_import_object_destroy(wasmer_import_object_t *import_object);

wasmer_import_object_t *wasmer_import_object_new(void);

wasmer_call_result_t wasmer_instance_call(wasmer_instance_t *instance,
                                          const char *name,
                                          const uint32_t *params,
                                          int params_len,
                                          uint32_t *results,
                                          int results_len);

void wasmer_instance_destroy(wasmer_instance_t *instance);

wasmer_compile_result_t wasmer_instantiate(wasmer_instance_t **instance,
                                           uint8_t *wasm_bytes,
                                           uint32_t wasm_bytes_len,
                                           wasmer_import_object_t *import_object);
