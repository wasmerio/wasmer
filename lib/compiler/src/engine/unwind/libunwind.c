#include <stdbool.h>
#include <stddef.h>

// Rust does not support weak imports on stable. A weak reference lets the final
// link select the unwinder, including in static executables without dlsym.
// Do not define a fallback symbol or force an unwinder library into the link.
extern void __unw_add_dynamic_fde(void*) __attribute__((weak));

__attribute__((visibility("hidden"))) bool wasmer_using_libunwind(void) {
  return __unw_add_dynamic_fde != NULL;
}
