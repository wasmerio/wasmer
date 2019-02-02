#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

typedef enum {
  WASMER_COMPILE_OK = 1,
  WASMER_COMPILE_ERROR = 2,
} wasmer_compile_result_t;

typedef struct wasmer_import_object_t wasmer_import_object_t;

typedef struct wasmer_instance_t wasmer_instance_t;

void wasmer_import_object_destroy(wasmer_import_object_t *import_object);

wasmer_import_object_t *wasmer_import_object_new(void);

wasmer_compile_result_t wasmer_instantiate(wasmer_instance_t *instance,
                                           const char *bytes,
                                           wasmer_import_object_t *import_object);
