.globl _run_on_wasm_stack
_run_on_wasm_stack:
# (stack_end, stack_begin)
# We need to ensure 16-byte alignment here.
pushq %r15
pushq %r14
pushq %r13
pushq %r12
pushq %rbx
pushq %rbp
movq %rsp, -16(%rdi)

leaq _run_on_wasm_stack.returning(%rip), %rax
movq %rax, -24(%rdi)

movq %rsi, %rsp
popq %rbp
popq %rbx
popq %r12
popq %r13
popq %r14
popq %r15
int $0x3
retq

_run_on_wasm_stack.returning:
movq (%rsp), %rsp
popq %rbp
popq %rbx
popq %r12
popq %r13
popq %r14
popq %r15
retq
