#ifndef NAME_SUFFIX
#define NAME_SUFFIX 0
#endif

#define CAT(a, b) CAT_INNER(a, b)
#define CAT_INNER(a, b) a##b

// Per-module TLS ensures __wasix_init_tls is exported and replayed on spawn.
_Thread_local int CAT(side_tls_, NAME_SUFFIX) = NAME_SUFFIX;

// Unique entry point per copy so main can dlopen/dlsym each module.
int CAT(side_touch_, NAME_SUFFIX)(void) {
  CAT(side_tls_, NAME_SUFFIX) = 1;
  return (int)__builtin_wasm_tls_base();
}
