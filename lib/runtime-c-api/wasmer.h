#include <cstdarg>
#include <cstdint>
#include <cstdlib>

enum class wasmer_compile_result_t {
  WASMER_COMPILE_OK = 1,
  WASMER_COMPILE_ERROR = 2,
};

struct wasmer_import_object_t;

struct wasmer_instance_t;

extern "C" {

void wasmer_import_object_destroy(wasmer_import_object_t *import_object);

wasmer_import_object_t *wasmer_import_object_new();

wasmer_compile_result_t wasmer_instantiate(wasmer_instance_t *instance,
                                           const char *bytes,
                                           wasmer_import_object_t *import_object);

} // extern "C"
