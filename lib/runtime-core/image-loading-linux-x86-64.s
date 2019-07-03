# NOTE: Keep this consistent with `fault.rs`.

.globl run_on_alternative_stack
run_on_alternative_stack:
# (stack_end, stack_begin)
# We need to ensure 16-byte alignment here.
pushq %r15
pushq %r14
pushq %r13
pushq %r12
pushq %rbx
pushq %rbp
movq %rsp, -16(%rdi)

leaq run_on_alternative_stack.returning(%rip), %rax
movq %rax, -24(%rdi)

movq %rsi, %rsp

movq (%rsp), %xmm0
add $8, %rsp

movq (%rsp), %xmm1
add $8, %rsp

movq (%rsp), %xmm2
add $8, %rsp

movq (%rsp), %xmm3
add $8, %rsp

movq (%rsp), %xmm4
add $8, %rsp

movq (%rsp), %xmm5
add $8, %rsp

movq (%rsp), %xmm6
add $8, %rsp

movq (%rsp), %xmm7
add $8, %rsp

popq %rbp
popq %rax
popq %rbx
popq %rcx
popq %rdx
popq %rdi
popq %rsi
popq %r8
popq %r9
popq %r10
popq %r11
popq %r12
popq %r13
popq %r14
popq %r15
retq

run_on_alternative_stack.returning:
movq (%rsp), %rsp
popq %rbp
popq %rbx
popq %r12
popq %r13
popq %r14
popq %r15
retq
