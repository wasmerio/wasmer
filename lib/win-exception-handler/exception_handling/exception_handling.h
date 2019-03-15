#ifndef WASMER_EXCEPTION_HANDLING_H
#define WASMER_EXCEPTION_HANDLING_H

#include <stdint.h>

struct func_t;
struct wasmer_instance_context_t;

typedef void(*trampoline_t)(struct wasmer_instance_context_t*,  const struct func_t*, const uint64_t*, uint64_t*);

struct call_protected_result_t {
    uint64_t code;
    uint64_t exception_address;
    uint64_t instruction_pointer;
};

uint8_t callProtected(
        trampoline_t trampoline,
        const struct wasmer_instance_context_t* ctx,
        const struct func_t* func,
        const uint64_t* param_vec,
        uint64_t* return_vec,
        struct call_protected_result_t* out_result);

#endif //WASMER_EXCEPTION_HANDLING_H
