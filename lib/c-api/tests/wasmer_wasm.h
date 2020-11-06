// This header file is used only for test purposes! It is used by unit
// test inside the `src/` directory for the moment.

#if !defined(TEST_WASMER_WASM)

#define TEST_WASMER_WASM

#include <string.h>
#include <stdio.h>
#include "../wasmer_wasm.h"

// Ideally, we want to use `assert.h`, but when compiling the C/C++
// code in release mode, the `assert` function calls are removed. We
// want them everytime, so let's reimplement something close!
//
// The following code is copy-pasted and adapted from glibc's
// `assert.h` file.

#if defined __cplusplus
# define __ASSERT_VOID_CAST static_cast<void>
#else
# define __ASSERT_VOID_CAST (void)
#endif

void _wasmer_assert_fail(const char* assertion, const char *file, unsigned int line, const char* function) {
  fprintf(
    stderr,
    "Assertion `%s` has failed, in `%s` at %s:%d\n",
    assertion,
    function,
    file,
    line
  );
}

// When possible, define assert so that it does not add extra
// parentheses around EXPR.  Otherwise, those added parentheses would
// suppress warnings we'd expect to be detected by gcc's
// -Wparentheses.
#if defined(__cplusplus)
#  define wasmer_assert(expr) \
     (static_cast<bool>(expr) \
       ? void(0) \
       : _wasmer_assert_fail(#expr, __FILE__, __LINE__, __ASSERT_FUNCTION))
#elif !defined(__GNUC__) || defined(__STRICT_ANSI__)
#  define wasmer_assert(expr) \
     ((expr) \
       ? __ASSERT_VOID_CAST(0) \
       : _wasmer_assert_fail(#expr, __FILE__, __LINE__, __ASSERT_FUNCTION))
#else
// The first occurrence of EXPR is not evaluated due to the sizeof,
// but will trigger any pedantic warnings masked by the __extension__
// for the second occurrence.  The ternary operator is required to
// support function pointers and bit fields in this context, and to
// suppress the evaluation of variable length arrays.
#  define wasmer_assert(expr) \
     ((void) sizeof ((expr) ? 1 : 0), __extension__ ({ \
       if (expr) \
         ; /* empty */ \
       else                                                           \
         _wasmer_assert_fail(#expr, __FILE__, __LINE__, __ASSERT_FUNCTION); \
     }))
#endif

// Version 2.4 and later of GCC define a magical variable
// `__PRETTY_FUNCTION__' which contains the name of the function
// currently being defined.  This is broken in G++ before version 2.6.
// C9x has a similar variable called __func__, but prefer the GCC one
// since it demangles C++ function names.
#if defined(__cplusplus)
#  define __ASSERT_FUNCTION __extension__ __PRETTY_FUNCTION__
#else
#  if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 199901L
#    define __ASSERT_FUNCTION __func__
#  else
#    define __ASSERT_FUNCTION ((const char *) 0)
#  endif
#endif

// Wasmer-specific shortcut to quickly create a `wasm_byte_vec_t` from
// a string.
static inline void wasm_byte_vec_new_from_string(
  wasm_byte_vec_t* out, const char* s
) {
  wasm_byte_vec_new(out, strlen(s), s);
}

#endif /* TEST_WASMER_WASM */
