# NOTE: Keep this consistent with `fault.rs`.

.globl _run_on_alternative_stack
_run_on_alternative_stack:
# (stack_end, stack_begin)
# We need to ensure 16-byte alignment here.
pushq %r15
pushq %r14
pushq %r13
pushq %r12
pushq %rbx
pushq %rbp
movq %rsp, -16(%rdi)

leaq _run_on_alternative_stack.returning(%rip), %rax
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

_run_on_alternative_stack.returning:
movq (%rsp), %rsp
popq %rbp
popq %rbx
popq %r12
popq %r13
popq %r14
popq %r15
retq

# For switching into a backend without information about where registers are preserved.
.globl _register_preservation_trampoline
_register_preservation_trampoline:
subq $8, %rsp
pushq %rax
pushq %rcx
pushq %rdx
pushq %rdi
pushq %rsi
pushq %r8
pushq %r9
pushq %r10

callq _get_boundary_register_preservation

# Keep this consistent with BoundaryRegisterPreservation
movq %r15, 0(%rax)
movq %r14, 8(%rax)
movq %r13, 16(%rax)
movq %r12, 24(%rax)
movq %rbx, 32(%rax)

popq %r10
popq %r9
popq %r8
popq %rsi
popq %rdi
popq %rdx
popq %rcx
popq %rax
addq $8, %rsp

jmpq *%rax
